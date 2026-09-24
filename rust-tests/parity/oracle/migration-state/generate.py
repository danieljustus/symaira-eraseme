#!/usr/bin/env python3
"""Pinned production Go Run oracle. No third-party modules or network required."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
PIN = '4e582f284a639bdaa01260b6b8ab6000482e6c0d'
SOURCES = ['internal/migration/migration.go', 'internal/scheduler/scheduler.go']


def cases():
    result = []
    valid = '{"version":1,"source_root":"@ROOT@/source","destination_root":"@ROOT@/destination","backup_dir":"@ROOT@/backup"'
    def add(name, state=None, marker=None):
        def encoded(value):
            return None if value is None else (value.encode() if isinstance(value, str) else value).hex()
        result.append(dict(id=name, state_hex=encoded(state), marker_hex=encoded(marker)))
    add('fresh')
    malformed = ['', ' ', '{', '[', '"', '"abc', '{"version":', '{"version":1', '{"version":1,}', '{x:1}', '{"x" 1}', '[1,]', '[1}', '{"x":1]', '{} {}', '{}\r\nfalse', '{\r\n"version": 1,\r\n"items": [1,}\r\n}', '{"x":"\n"}', '{"x":"\r"}', '{"x":"\\q"}', '{"x":"\\u0X00"}', '-', '-x', '1.', '1.x', '1e', '1e+', '1e-x', '01', 'tru', 'truX', 'nulX', 'falsX', '\ufeff{}', '{"version":"bad","x":}', '{"x":true false}', "{'x':1}"]
    for i, data in enumerate(malformed):
        add(f'syntax-{i:02}', data)
        add(f'marker-syntax-{i:02}', None, data)
    for data in ['null', '[]', '[1]', 'true', 'false', '1', '-1', '1.0', '1e999', '"text"', '{}']:
        add('top-'+data, data)
        add('marker-top-'+data, None, data)
    fields = {'version':'1', 'source_root':'"@ROOT@/source"', 'destination_root':'"@ROOT@/destination"', 'backup_dir':'"@ROOT@/backup"', 'items':'{"config:config.toml":"done","retained":"old"}'}
    def obj(values): return '{'+','.join('"'+k+'":'+v for k,v in values)+'}'
    for field, correct in fields.items():
        other = [(k,v) for k,v in fields.items() if k != field]
        add(field+'-omitted', obj(other))
        for label, value in [('null','null'),('string','"bad"'),('bool','true'),('number','2'),('array','[]'),('object','{}')]:
            add(field+'-'+label, obj(other+[(field,value)]))
            add(field+'-duplicate-'+label, obj(other+[(field,value),(field,correct)]))
        add(field+'-duplicate-null-after', obj(other+[(field,correct),(field,'null')]))
        add(field+'-case',obj(other+[(field.upper(),correct)]))
    for n in ['-9223372036854775809','-9223372036854775808','-2147483649','-1','0','2147483647','2147483648','4294967296','9223372036854775807','9223372036854775808','1.0','1e0','1e999','-0']:
        add('version-boundary-'+n,obj([(k,v) for k,v in fields.items() if k!='version']+[('version',n)]))
    for label, suffix in [
        ('merge','"items":{"config:config.toml":"done","a":"one"},"items":{"b":"two"}'),
        ('clear','"items":{"config:config.toml":"done"},"items":null'),
        ('clear-then-merge','"items":{"a":"one"},"items":null,"items":{"b":"two"}'),
        ('value-null','"items":{"config:config.toml":"done","config:config.toml":null}'),
        ('merge-value-null','"items":{"config:config.toml":"done"},"items":{"config:config.toml":null}'),
        ('value-error','"items":{"config:config.toml":3,"config:config.toml":"done"}'),
        ('unicode','"items":{"\\ud800":"\\udc00","pair":"\\ud83d\\ude00","escape":"<&>\\u2028\\u2029"}'),
        ('unicode-fold','"itemſ":{"config:config.toml":"done"}'),
        ('unknown','"unknown":{"deep":[null,true,1e999]},"SourceRoot":"ignored"'),
        ('first-error','"items":{"a":true},"version":false'),
    ]: add('items-'+label, valid+','+suffix+'}')
    for label, raw in [('invalid-byte',b'\xff'),('truncated-utf8',b'\xe2\x82'),('surrogate-utf8',b'\xed\xa0\x80'),('bad-continuation',b'\xe2(\xa1')]:
        add('unicode-'+label,valid.encode()+b',"items":{"'+raw+b'":"'+raw+b'"}}')
    for key, right in [('version','1'),('source','"@ROOT@/source"')]:
        other=('"source":"@ROOT@/source"' if key=='version' else '"version":1')
        for label, value in [('null','null'),('bool','true'),('number','3'),('string','"bad"'),('array','[]'),('object','{}')]:
            add('marker-'+key+'-'+label,None,'{'+other+',"'+key+'":'+value+'}')
            add('marker-'+key+'-duplicate-'+label,None,'{'+other+',"'+key+'":'+value+',"'+key+'":'+right+'}')
        add('marker-'+key+'-null-after',None,'{'+other+',"'+key+'":'+right+',"'+key+'":null}')
    add('marker-case',None,'{"VERSION":1,"SOURCE":"@ROOT@/source"}')
    add('marker-valid',None,'{"version":1,"source":"@ROOT@/source"}')
    add('marker-valid-resume',valid+'}', '{"version":1,"source":"@ROOT@/source"}')
    for depth in [128,9999,10000,10001]: add('depth-'+str(depth),valid+',"unknown":'+'['*depth+'0'+']'*depth+'}')
    add('kelvin-fold',valid.replace('backup_dir','bacKup_dir')+'}')
    add('marker-long-s-fold',None,'{"version":1,"ſource":"@ROOT@/source"}')
    for i in range(256):
        add(f'syntax-byte-{i:02x}',bytes([i]))
    for label, data in [('escaped-eof',b'"\\'),('unicode-eof',b'"\\u12'),('key-eof',b'{"x"'),('fraction-eof',b'{"x":1.'),('keyword-eof',b'{"x":fa')]:
        add('truncated-'+label,data)
    for kind in ['true', '1', '[]', '{}']:
        add('map-element-'+kind,valid+',"items":{"a":'+kind+'}}')
    assert len({c['id'] for c in result})==len(result)>0
    return result


def run(cmd, cwd, env, data=None):
    # The probe has no subprocess/network/credential operations. Still bound its group.
    p=subprocess.Popen(cmd,cwd=cwd,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
    try: out,err=p.communicate(data,timeout=60)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGKILL); p.communicate(timeout=5); raise
    if p.returncode: raise RuntimeError((cmd,p.returncode,out,err))
    assert not err,err
    return out


def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--go',required=True); parser.add_argument('--check',action='store_true'); parser.add_argument('--fixture',type=Path,default=HERE/'fixture.json'); args=parser.parse_args()
    target=REPO/'target'; target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='migration-state-oracle-',dir=target) as scratch:
        scratch=Path(scratch); module=scratch/'module'; module.mkdir(); home=scratch/'home'; home.mkdir()
        env={'PATH':os.environ['PATH'],'HOME':str(home),'USERPROFILE':str(home),'TMPDIR':str(scratch),'TMP':str(scratch),'TEMP':str(scratch),'GOCACHE':str(target/'migration-state-gocache'),'GOTOOLCHAIN':'local','GOPROXY':'off','GOSUMDB':'off','GOWORK':'off','GOENV':'off','CGO_ENABLED':'0','LANG':'C','LC_ALL':'C','TZ':'UTC'}
        for key in ['XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']: env[key]=str(home/key)
        version=run([args.go,'version'],module,env).decode().strip(); assert version.startswith('go version go1.26.6 '),version
        identity={}
        for name in SOURCES:
            data=subprocess.check_output(['git','show',PIN+':'+name],cwd=REPO)
            if name==SOURCES[0]: assert data==(REPO/name).read_bytes(),'production migration drift'
            path=module/name; path.parent.mkdir(parents=True,exist_ok=True); path.write_bytes(data)
            identity[name]=hashlib.sha256(data).hexdigest()
        (module/'go.mod').write_text('module github.com/danieljustus/symaira-eraseme\n\ngo 1.26.6\n')
        (module/'main.go').write_bytes((HERE/'probe.go').read_bytes())
        binary=scratch/'probe'; run([args.go,'build','-trimpath','-o',str(binary),'.'],module,env)
        results=[]
        for case in cases():
            root=scratch/('case-'+str(len(results))); root.mkdir()
            isolated=dict(env)
            for key in ['HOME','USERPROFILE','TMPDIR','TMP','TEMP','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']: isolated[key]=str(root/'home'/key); Path(isolated[key]).mkdir(parents=True,exist_ok=True)
            raw=run([str(binary)],root,isolated,json.dumps(case).encode())
            results.append(dict(**case,raw_stdout_hex=raw.hex(),expected=json.loads(raw)))
        artifact=dict(schema=1,source_revision=PIN,source_sha256=identity,toolchain=version,build_command=['go1.26.6','build','-trimpath','-o','<scratch>/probe','.'],generated_on=version.split()[-1],generator_sha256={n:hashlib.sha256((HERE/n).read_bytes()).hexdigest() for n in ['generate.py','probe.go']},declared_ids=[c['id'] for c in cases()],cases=results)
        output=(json.dumps(artifact,ensure_ascii=True,sort_keys=True,indent=2)+'\n').encode()
        if args.check:
            assert args.fixture.read_bytes()==output,'oracle fixture differs (check mode did not rewrite)'
        else: args.fixture.write_bytes(output)
        print(('checked' if args.check else 'generated'),len(results),'cases',hashlib.sha256(output).hexdigest())

if __name__=='__main__': main()
