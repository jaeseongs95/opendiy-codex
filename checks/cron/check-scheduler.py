"""Compile the production scheduler module and prove the stale-attempt test detects a mutation."""
from pathlib import Path
import argparse,hashlib,json,subprocess
parser=argparse.ArgumentParser(); parser.add_argument('--rustc',required=True); args=parser.parse_args()
root=Path(__file__).resolve().parent
source=root.parents[1]/'codex-rs/tui/src/cron.rs'
def run(name,command):
    completed=subprocess.run(command,capture_output=True,text=True,encoding='utf-8')
    (root/(name+'.log')).write_text(completed.stdout+completed.stderr,encoding='utf-8')
    return completed
exe=root/'cron-scheduler-tests.exe'
build=run('scheduler-build',[args.rustc,'--edition','2024','--test',str(source),'-o',str(exe)])
assert build.returncode==0,build.stderr
green=run('scheduler-test',[str(exe),'--nocapture']); assert green.returncode==0,green.stdout
text=source.read_text(encoding='utf-8'); guard='if *expected != attempt {'; assert text.count(guard)==1
mutation=root/'mutation-scheduler.rs'; mutation.write_text(text.replace(guard,'if false {',1),encoding='utf-8')
mutant_exe=root/'mutation-scheduler-tests.exe'
mutant_build=run('mutation-build',[args.rustc,'--edition','2024','--test',str(mutation),'-o',str(mutant_exe)])
assert mutant_build.returncode==0,mutant_build.stderr
red=run('mutation-test',[str(mutant_exe),'--nocapture'])
assert red.returncode!=0 and 'completion_before_reply_duplicate_reply_and_remove_preserve_exact_owner ... FAILED' in red.stdout,red.stdout
receipt={'source':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'rustc':args.rustc,'buildExitCode':build.returncode,'testExitCode':green.returncode,'mutation':'remove stale attempt guard only; tests unchanged','mutationBuildExitCode':mutant_build.returncode,'mutationTestExitCode':red.returncode,'proof':'green production / red guard mutation; same actual module tests','integrationTests':'NOT_RUN'}
(root/'scheduler-receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(green.stdout.strip());print('mutation detected; exit='+str(red.returncode))
