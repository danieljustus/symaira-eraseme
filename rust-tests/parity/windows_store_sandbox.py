"""Native Windows boundary for disposable migration CLI observations.

No capabilities are granted to the AppContainer. Only the fresh evidence root
receives its unique SID; child creation is forbidden. A kill-on-close Job owns
the suspended process before any code executes. Failures retain raw evidence.
"""
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import subprocess
import time
import uuid

P = c.c_void_p
SIZE = c.c_size_t
DWORD = w.DWORD


class Startup(c.Structure):
    _fields_ = [('cb', DWORD), ('reserved', w.LPWSTR), ('desktop', w.LPWSTR),
                ('title', w.LPWSTR), ('x', DWORD), ('y', DWORD), ('sx', DWORD),
                ('sy', DWORD), ('cx', DWORD), ('cy', DWORD), ('fill', DWORD),
                ('flags', DWORD), ('show', w.WORD), ('reserved_size', w.WORD),
                ('reserved_bytes', P), ('stdin', w.HANDLE), ('stdout', w.HANDLE),
                ('stderr', w.HANDLE)]


class StartupEx(c.Structure):
    _fields_ = [('startup', Startup), ('attributes', P)]


class Process(c.Structure):
    _fields_ = [('process', w.HANDLE), ('thread', w.HANDLE),
                ('pid', DWORD), ('tid', DWORD)]


class Capabilities(c.Structure):
    _fields_ = [('sid', P), ('capabilities', P), ('count', DWORD), ('reserved', DWORD)]


class BasicLimits(c.Structure):
    _fields_ = [('process_time', c.c_int64), ('job_time', c.c_int64),
                ('flags', DWORD), ('min_working', SIZE), ('max_working', SIZE),
                ('active_limit', DWORD), ('affinity', SIZE),
                ('priority', DWORD), ('scheduling', DWORD)]


class IoCounters(c.Structure):
    _fields_ = [(name, c.c_uint64) for name in
                ('read_ops', 'write_ops', 'other_ops', 'read_bytes', 'write_bytes', 'other_bytes')]


class ExtendedLimits(c.Structure):
    _fields_ = [('basic', BasicLimits), ('io', IoCounters),
                ('process_memory', SIZE), ('job_memory', SIZE),
                ('peak_process', SIZE), ('peak_job', SIZE)]


class Accounting(c.Structure):
    _fields_ = [('user_time', c.c_int64), ('kernel_time', c.c_int64),
                ('period_user', c.c_int64), ('period_kernel', c.c_int64),
                ('faults', DWORD), ('total', DWORD), ('active', DWORD), ('terminated', DWORD)]


def api(dll, name, args, result=w.BOOL):
    fn = getattr(dll, name)
    fn.argtypes, fn.restype = args, result
    return fn


def checked(value, label):
    if not value:
        raise c.WinError(c.get_last_error(), label)
    return value


def command(root, label, argv, env, timeout=30):
    import plain_store_switchback as gate
    root = Path(root)
    record = {'argv': list(map(str, argv)), 'cwd': str(root), 'exit_code': None,
              'timed_out': False, 'output_limit_exceeded': False, 'success': False,
              'sandbox': {'mechanism': 'native AppContainer without capabilities and Job Object',
                          'capability_count': 0, 'child_creation': 'restricted',
                          'writable_root': str(root), 'job_active_after_cleanup': None,
                          'profile_deleted': False, 'root_sid_removed': False}}
    try:
        return _command(root, label, argv, env, timeout, record)
    except BaseException as error:
        record['success'] = False
        record['failure_class'] = type(error).__name__
        raise
    finally:
        for stream in ('stdout', 'stderr'):
            path = root / (label + '.' + stream)
            if path.is_file():
                record[stream] = {'path': path.name, **gate.identity(path)}
        gate.save(root / (label + '.json'), record)


def _command(root, label, argv, env, timeout, record):
    if os.name != 'nt':
        raise ValueError('native Windows is required')
    import msvcrt
    root = Path(root).resolve(strict=True)
    executable = Path(argv[0]).resolve(strict=True)
    if root.is_symlink() or not executable.is_relative_to(root) or executable.suffix.lower() != '.exe':
        raise ValueError('Windows sandbox executes only staged .exe files under its owned root')
    if timeout <= 0 or timeout > 30:
        raise ValueError('Windows command deadline must be in (0, 30] seconds')
    kernel = c.WinDLL('kernel32', use_last_error=True)
    userenv = c.WinDLL('userenv', use_last_error=True)
    advapi = c.WinDLL('advapi32', use_last_error=True)
    create_profile = api(userenv, 'CreateAppContainerProfile',
                         [w.LPCWSTR, w.LPCWSTR, w.LPCWSTR, P, DWORD, c.POINTER(P)], c.c_long)
    delete_profile = api(userenv, 'DeleteAppContainerProfile', [w.LPCWSTR], c.c_long)
    free_sid = api(advapi, 'FreeSid', [P], P)
    sid_text = api(advapi, 'ConvertSidToStringSidW', [P, c.POINTER(w.LPWSTR)])
    local_free = api(kernel, 'LocalFree', [P], P)
    create_job = api(kernel, 'CreateJobObjectW', [P, w.LPCWSTR], w.HANDLE)
    set_job = api(kernel, 'SetInformationJobObject', [w.HANDLE, c.c_int, P, DWORD])
    query_job = api(kernel, 'QueryInformationJobObject', [w.HANDLE, c.c_int, P, DWORD, P])
    assign_job = api(kernel, 'AssignProcessToJobObject', [w.HANDLE, w.HANDLE])
    terminate_job = api(kernel, 'TerminateJobObject', [w.HANDLE, w.UINT])
    terminate_process = api(kernel, 'TerminateProcess', [w.HANDLE, w.UINT])
    resume = api(kernel, 'ResumeThread', [w.HANDLE], DWORD)
    wait = api(kernel, 'WaitForSingleObject', [w.HANDLE, DWORD], DWORD)
    exit_code = api(kernel, 'GetExitCodeProcess', [w.HANDLE, c.POINTER(DWORD)])
    close = api(kernel, 'CloseHandle', [w.HANDLE])
    inherit = api(kernel, 'SetHandleInformation', [w.HANDLE, DWORD, DWORD])
    init_attrs = api(kernel, 'InitializeProcThreadAttributeList', [P, DWORD, DWORD, c.POINTER(SIZE)])
    update_attr = api(kernel, 'UpdateProcThreadAttribute', [P, DWORD, SIZE, P, SIZE, P, P])
    delete_attrs = api(kernel, 'DeleteProcThreadAttributeList', [P], None)
    create_process = api(kernel, 'CreateProcessW',
                          [w.LPCWSTR, w.LPWSTR, P, P, w.BOOL, DWORD, P,
                           w.LPCWSTR, c.POINTER(StartupEx), c.POINTER(Process)])
    profile = 'eraseme-migration-' + uuid.uuid4().hex
    sid, text_sid, job, process = P(), w.LPWSTR(), None, Process()
    attrs_ready, profile_created, granted = False, False, False
    record['sandbox']['profile'] = profile
    attrs = None
    try:
        hr = create_profile(profile, profile, 'Disposable EraseMe migration fixture', None, 0, c.byref(sid))
        if hr != 0:
            raise OSError('CreateAppContainerProfile failed: ' + hex(hr & 0xffffffff))
        profile_created = True
        checked(sid_text(sid, c.byref(text_sid)), 'ConvertSidToStringSidW')
        sid_string = text_sid.value
        # The parent only changes ACLs on the newly created, caller-owned root.
        icacls = Path(os.environ['SystemRoot']) / 'System32/icacls.exe'
        def acl(*args):
            subprocess.run([str(icacls), str(root), *args, '/T', '/Q'],
                           stdin=subprocess.DEVNULL, capture_output=True, check=True, timeout=30)
        granted = True  # Even a partial icacls failure must remove the exact owned SID.
        acl('/grant', '*' + sid_string + ':(OI)(CI)F')
        acl('/setintegritylevel', '(OI)(CI)L')
        job = checked(create_job(None, None), 'CreateJobObjectW')
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        checked(set_job(job, 9, c.byref(limits), c.sizeof(limits)), 'SetInformationJobObject')
        size = SIZE()
        init_attrs(None, 3, 0, c.byref(size))
        if not size.value:
            raise c.WinError(c.get_last_error(), 'attribute list size')
        attrs = c.create_string_buffer(size.value)
        checked(init_attrs(attrs, 3, 0, c.byref(size)), 'InitializeProcThreadAttributeList')
        attrs_ready = True
        caps = Capabilities(sid, None, 0, 0)
        # PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES; zero network capabilities.
        checked(update_attr(attrs, 0, 0x20009, c.byref(caps), c.sizeof(caps), None, None), 'security capabilities')
        policy = DWORD(1)  # PROCESS_CREATION_CHILD_PROCESS_RESTRICTED
        checked(update_attr(attrs, 0, 0x2000e, c.byref(policy), c.sizeof(policy), None, None), 'child process restriction')
        with open(os.devnull, 'rb') as inp, (root / (label + '.stdout')).open('xb') as out, \
                (root / (label + '.stderr')).open('xb') as err:
            handles = (w.HANDLE * 3)(*(msvcrt.get_osfhandle(stream.fileno()) for stream in (inp, out, err)))
            for handle in handles:
                checked(inherit(handle, 1, 1), 'enable selected handle inheritance')
            try:
                checked(update_attr(attrs, 0, 0x20002, handles, c.sizeof(handles), None, None), 'explicit handle list')
                startup = StartupEx()
                startup.startup.cb = c.sizeof(startup)
                startup.startup.flags = 0x100  # STARTF_USESTDHANDLES
                startup.startup.stdin, startup.startup.stdout, startup.startup.stderr = handles
                startup.attributes = c.cast(attrs, P)
                environment = c.create_unicode_buffer('\0'.join(k + '=' + str(v) for k, v in sorted(env.items())) + '\0\0')
                line = c.create_unicode_buffer(subprocess.list2cmdline(list(map(str, argv))))
                checked(create_process(str(executable), line, None, None, True,
                                       0x80000 | 0x400 | 0x4, environment, str(root),
                                       c.byref(startup), c.byref(process)), 'CreateProcessW suspended AppContainer')
            finally:
                for handle in handles:
                    checked(inherit(handle, 1, 0), 'disable selected handle inheritance')
            # Assignment failure never resumes the unowned suspended process.
            checked(assign_job(job, process.process), 'AssignProcessToJobObject')
            if resume(process.thread) == 0xffffffff:
                raise c.WinError(c.get_last_error(), 'ResumeThread')
            deadline = time.monotonic() + timeout
            while True:
                status = wait(process.process, 25)
                if status == 0:
                    break
                if status != 258:
                    raise c.WinError(c.get_last_error(), 'WaitForSingleObject')
                if any((root / (label + '.' + name)).stat().st_size > 4 * 1024 * 1024 for name in ('stdout', 'stderr')):
                    record['output_limit_exceeded'] = True
                    break
                if time.monotonic() >= deadline:
                    record['timed_out'] = True
                    break
            code = DWORD()
            checked(exit_code(process.process, c.byref(code)), 'GetExitCodeProcess')
            record['exit_code'] = code.value
            record['output_limit_exceeded'] = record['output_limit_exceeded'] or any(
                (root / (label + '.' + name)).stat().st_size > 4 * 1024 * 1024 for name in ('stdout', 'stderr'))
    finally:
        try:
            if process.process:
                # Terminate the job even when its leader already exited.
                if job:
                    checked(terminate_job(job, 124), 'TerminateJobObject')
                if wait(process.process, 0) == 258:
                    checked(terminate_process(process.process, 124), 'TerminateProcess owned suspended child')
                if wait(process.process, 5000) != 0:
                    raise RuntimeError('owned Windows process did not reap within five seconds')
                code = DWORD()
                checked(exit_code(process.process, c.byref(code)), 'GetExitCodeProcess after cleanup')
                record['exit_code'] = code.value
            if job:
                end = time.monotonic() + 5
                accounting = Accounting()
                while True:
                    checked(query_job(job, 1, c.byref(accounting), c.sizeof(accounting), None), 'Job accounting')
                    if accounting.active == 0:
                        break
                    if time.monotonic() >= end:
                        raise RuntimeError('Windows Job retained active processes after cleanup')
                    time.sleep(0.025)
                record['sandbox']['job_active_after_cleanup'] = accounting.active
        finally:
            for handle in (process.thread, process.process, job):
                if handle:
                    checked(close(handle), 'CloseHandle owned process/job')
            if attrs_ready:
                delete_attrs(attrs)
            try:
                if granted:
                    acl('/remove', '*' + sid_string)
                    record['sandbox']['root_sid_removed'] = True
            finally:
                if text_sid:
                    local_free(c.cast(text_sid, P))
                if sid:
                    free_sid(sid)
                if profile_created:
                    hr = delete_profile(profile)
                    if hr != 0:
                        raise OSError('DeleteAppContainerProfile failed: ' + hex(hr & 0xffffffff))
                    record['sandbox']['profile_deleted'] = True
                import plain_store_switchback as gate
                for stream in ('stdout', 'stderr'):
                    path = root / (label + '.' + stream)
                    if path.is_file():
                        record[stream] = {'path': path.name, **gate.identity(path)}
                record['success'] = (record['exit_code'] == 0 and not record['timed_out']
                                     and not record['output_limit_exceeded']
                                     and record['sandbox']['job_active_after_cleanup'] == 0
                                     and record['sandbox']['profile_deleted']
                                     and record['sandbox']['root_sid_removed'])
                gate.save(root / (label + '.json'), record)
    if not record['success']:
        raise ValueError(label + ': native Windows command failed; retained raw evidence')
    return record
