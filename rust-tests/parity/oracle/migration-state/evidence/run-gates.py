import os,json,subprocess,pathlib,time,hashlib
r=pathlib.Path.cwd(); e=r/'target/json-evidence'; env=dict(os.environ)
env.update(CARGO_TARGET_DIR=str(r/'target'),CARGO_NET_OFFLINE='true',CARGO_HOME=os.environ.get('CARGO_HOME',str(pathlib.Path.home()/'.cargo')),RUSTUP_HOME=os.environ.get('RUSTUP_HOME',str(pathlib.Path.home()/'.rustup')),GOCACHE=str(r/'target/migration-state-gocache'),GOTOOLCHAIN='local',GOPROXY='off',GOSUMDB='off',GOENV='off',GOWORK='off',PATH='/Users/daniel/sdk/go1.26.6/bin:'+os.environ['PATH'])
for key in ['HOME','USERPROFILE','TMPDIR','TMP','TEMP','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']:
 p=e/'isolated'/key;p.mkdir(parents=True,exist_ok=True);env[key]=str(p)
assert subprocess.run(['python3','-c','raise SystemExit(7)'],env=env).returncode==7
meta=subprocess.check_output(['cargo','metadata','--manifest-path',str(r/'Cargo.toml'),'--no-deps','--format-version','1','--locked','--offline'],env=env,cwd=r)
m=json.loads(meta);assert m['workspace_root']==str(r) and m['target_directory']==str(r/'target')
for name in ['symeraseme-engine','symeraseme-cli']:
 p=next(p for p in m['packages'] if p['name']==name);assert p['manifest_path']==str(r/'crates'/name/'Cargo.toml');assert all(pathlib.Path(t['src_path']).is_relative_to(r) for t in p['targets'])
(e/'metadata-final.json').write_bytes(meta)
commands=[('engine-tests',['cargo','test','--manifest-path',str(r/'Cargo.toml'),'-p','symeraseme-engine','--locked','--offline','--','--nocapture']),('clippy',['cargo','clippy','--manifest-path',str(r/'Cargo.toml'),'-p','symeraseme-engine','-p','symeraseme-cli','--all-targets','--all-features','--locked','--offline','--','-D','warnings']),('fmt',['cargo','fmt','--manifest-path',str(r/'Cargo.toml'),'--all','--','--check']),('diff',['git','diff','--check'])]
results=[]
for name,cmd in commands:
 with (e/(name+'.log')).open('wb') as f:
  result=subprocess.run(cmd,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
 results.append(dict(name=name,command=cmd,exit=result.returncode))
 (e/'gates.json').write_text(json.dumps(results,indent=2)+'\n')
 print(name,result.returncode,flush=True)
raise SystemExit(any(x['exit'] for x in results))
