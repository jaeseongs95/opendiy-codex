from pathlib import Path
import hashlib,json,os,queue,subprocess,threading,time
root=Path(__file__).resolve().parent
home=root/'old-backend-home'; home.mkdir(parents=True,exist_ok=True)
exe=Path(os.environ['APPDATA'])/'npm/node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe'
assert exe.is_file(),exe
env=os.environ.copy(); env['CODEX_HOME']=str(home)
for key in ['OPENAI_API_KEY','CODEX_API_KEY']: env.pop(key,None)
version=subprocess.run([str(exe),'--version'],env=env,capture_output=True,text=True,timeout=15)
process=subprocess.Popen([str(exe),'app-server','--listen','stdio://'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,encoding='utf-8')
responses=queue.Queue(); records=[]; stderr=[]
def read_stdout():
    for line in process.stdout:
        records.append(line.rstrip()); responses.put(json.loads(line))
def read_stderr():
    for line in process.stderr: stderr.append(line.rstrip())
threading.Thread(target=read_stdout,daemon=True).start(); threading.Thread(target=read_stderr,daemon=True).start()
def send(value): process.stdin.write(json.dumps(value)+'\n'); process.stdin.flush()
def receive(request_id):
    deadline=time.monotonic()+20
    while time.monotonic()<deadline:
        item=responses.get(timeout=max(.1,deadline-time.monotonic()))
        if item.get('id')==request_id:return item
    raise TimeoutError(request_id)
try:
    send({'id':1,'method':'initialize','params':{'clientInfo':{'name':'cron-compat-check','version':'0.0.0'},'capabilities':{'experimentalApi':True}}})
    assert 'result' in receive(1)
    send({'method':'initialized'})
    send({'id':2,'method':'turn/startIfIdle','params':{'threadId':'00000000-0000-0000-0000-000000000000','input':[{'type':'text','text':'CRON_OLD_BACKEND_MUST_NOT_RUN','text_elements':[]}]}})
    reply=receive(2)
    assert reply.get('error',{}).get('code')==-32600,reply
    assert "unknown variant `turn/startIfIdle`" in reply['error']['message'],reply
    assert not any(json.loads(line).get('method') in ['turn/started','item/started'] for line in records)
finally:
    process.stdin.close()
    try:process.wait(timeout=10)
    except subprocess.TimeoutExpired:process.terminate();process.wait(timeout=5)
(root/'old-backend-stdio.jsonl').write_text('\n'.join(records)+'\n',encoding='utf-8')
summary={'exe':str(exe),'sha256':hashlib.sha256(exe.read_bytes()).hexdigest(),'version':version.stdout.strip(),'versionExitCode':version.returncode,'exitCode':process.returncode,'requestMethod':'turn/startIfIdle','error':reply['error'],'ordinaryTurnStartSent':0,'turnStartedObserved':0,'home':str(home),'authCopied':False}
(root/'old-backend-result.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(json.dumps({key:summary[key] for key in ['version','exitCode','ordinaryTurnStartSent','turnStartedObserved']},ensure_ascii=False))
print(reply['error']['message'][:180])
