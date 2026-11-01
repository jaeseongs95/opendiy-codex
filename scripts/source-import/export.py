"""Export pinned public Git blobs, not working files, operational checks, or Git history."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile

OFFICIAL='6ff1e1ca3b30fc8b914720681fcf5fe62e95f856'
PATCHED='5df31bc7f83e8427000a3672fb6ca48d22d8d2e8'
PREVIOUS_PUBLIC='69adce4def3cde379210f6eb63c66af5714ed19b'
PREFIXES=['codex-rs','third_party','patches','.cargo/config.toml','tools/windows-cli','.gitattributes','docs/native-cli-r27.ko.md','docs/native-cli-cron.ko.md']
PROTECTED=['LICENSE','NOTICE','README.md','README.ko.md']
DATE='2026-11-01'
PATCH_PATH='upstream/official-0.166.0.patch'
LABEL='R31'
ATTR_BEGIN=b'# BEGIN OpenDIY pinned source attributes\n'
ATTR_END=b'# END OpenDIY pinned source attributes\n'
WINDOWS_INPUTS_PATH='upstream/windows-prompt-inputs.json'
WINDOWS_INPUTS={p['path']:p for p in json.loads((Path(__file__).resolve().parents[2]/WINDOWS_INPUTS_PATH).read_bytes())['files']}

def sha(data):return hashlib.sha256(data).hexdigest()
def git(repository,*args):return subprocess.check_output(['git','-c','core.autocrlf=false','-C',str(repository),*args])
def encode(value):return (json.dumps(value,ensure_ascii=False,indent=2)+'\n').encode()

def notice(path):
    if path.startswith('codex-rs/app-server-protocol/schema/typescript/') and path.endswith('.ts'):return b''
    if path.endswith(('.rs','.ts')):prefix='//'
    elif path.endswith(('.py','.toml','Cargo.lock','.gitattributes','.gitignore')):prefix='#'
    elif path.endswith('.md') and path.startswith('docs/'):return f'<!-- Modified for OpenDIY Codex {LABEL} ({DATE}); see MODIFICATIONS.md. -->\n'.encode()
    else:return b''  # Data formats and snapshots stay exact; MODIFICATIONS.md lists each one.
    return f'{prefix} Modified for OpenDIY Codex {LABEL} ({DATE}); see MODIFICATIONS.md.\n'.encode()

def checked_target(root,path):
    relative=Path(path)
    target=root/relative
    if relative.is_absolute() or '..' in relative.parts or any(p in relative.parts for p in ['.git','target','node_modules','checks']) or not target.resolve().is_relative_to(root.resolve()):
        raise ValueError(f'Unsafe export target: {path}')
    if target.parent.resolve()!=root.resolve().joinpath(*relative.parts[:-1]):raise ValueError(f'Unexpected export parent symlink: {path}')
    if target.is_symlink() and (relative.as_posix()!='codex-rs/vendor/bubblewrap/LICENSE' or os.readlink(target)!='COPYING'):
        raise ValueError(f'Unexpected export symlink: {path}')
    return target

def canonical_bytes(relative,data):return data.replace(b'\r\n',b'\n') if relative in WINDOWS_INPUTS else data

def existing_bytes(path,relative):return canonical_bytes(relative,os.readlink(path).encode() if path.is_symlink() else path.read_bytes())

def prompt_attributes():
    return b'# Embedded prompt inputs use native checkout EOL; do not force Linux CRLF.\n'+''.join(f'{path} text\n' for path in WINDOWS_INPUTS).encode()

def publish(root,outputs,previous):
    # Check the whole update before changing anything. Only previously inventoried bytes may be replaced.
    for path,expected in previous.items():
        target=checked_target(root,path)
        if not target.is_file() or sha(existing_bytes(target,path))!=expected:raise ValueError(f'Previous import changed: {path}')
    for path,data in outputs.items():
        target=checked_target(root,path)
        if target.exists() and existing_bytes(target,path)!=data and path not in previous:raise ValueError(f'Existing file differs; refusing overwrite: {path}')
    for path,data in outputs.items():
        target=checked_target(root,path);target.parent.mkdir(parents=True,exist_ok=True)
        if not target.exists() or existing_bytes(target,path)!=data:target.write_bytes(data)
    for path in previous.keys()-outputs.keys():checked_target(root,path).unlink()

def pinned_previous_source(root,previous):
    # Later public test commits must be preserved in R31, never silently stamped.
    # Both their bytes and the import metadata are anchored to the reviewed Git commit.
    git(root,'diff','--quiet',PREVIOUS_PUBLIC,'--',*PREFIXES,'SOURCE_BASELINE.json','upstream/source-inventory.json')
    tracked=set(git(root,'ls-tree','-r','--name-only',PREVIOUS_PUBLIC,'--',*PREFIXES).decode().splitlines())
    previous=dict(previous)
    for path in tracked:
        target=checked_target(root,path)
        current=existing_bytes(target,path)
        if path not in previous or sha(current)!=previous[path]:
            pinned=canonical_bytes(path,git(root,'show',PREVIOUS_PUBLIC+':'+path))
            if current!=pinned:raise ValueError(f'Public source differs from reviewed commit: {path}')
            previous[path]=sha(pinned)
    return previous

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-repository',type=Path)
    parser.add_argument('--output',type=Path,default=Path(__file__).resolve().parents[2])
    parser.add_argument('--update',action='store_true',help='Replace only a verified previous source inventory; preserve repository documents')
    parser.add_argument('--stage-modes',action='store_true',help='Restore imported Git modes after git add; does not change file bytes')
    args=parser.parse_args();root=args.output.resolve();source=args.source_repository
    if args.stage_modes:
        inventory=json.loads((root/'upstream/source-inventory.json').read_bytes());staged={}
        for row in git(root,'ls-files','--stage','-z').split(b'\0'):
            if row:
                metadata,path=row.split(b'\t');staged[path.decode()]=metadata.decode().split()[1]
        rows=[]
        for item in inventory['files']:
            path=item['path'];data=existing_bytes(checked_target(root,path),path)
            if sha(data)!=item['publishedSha256'] or path not in staged:raise ValueError(f'Source not staged as reviewed: {path}')
            rows.append(f'{item["gitMode"]} {staged[path]}\t{path}\n')
        subprocess.run(['git','-C',str(root),'update-index','--index-info'],input=''.join(rows).encode(),check=True)
        print(json.dumps({'status':'PASS','stagedSourceModes':len(rows)}));return
    if source is None:parser.error('--source-repository is required for source export')
    if git(source,'rev-parse',OFFICIAL).decode().strip()!=OFFICIAL or git(source,'rev-parse',PATCHED).decode().strip()!=PATCHED:raise ValueError('Missing source commits')
    existing={p:sha((root/p).read_bytes()) for p in PROTECTED}
    previous={}
    if args.update:
        baseline=json.loads((root/'SOURCE_BASELINE.json').read_bytes())
        inventory_raw=(root/'upstream/source-inventory.json').read_bytes()
        if sha(inventory_raw)!=baseline['sourceInventorySha256']:raise ValueError('Previous inventory changed')
        previous={item['path']:item['publishedSha256'] for item in json.loads(inventory_raw)['files']}
        previous=pinned_previous_source(root,previous)
        if baseline.get('patchPath')==PATCH_PATH:previous[PATCH_PATH]=baseline['patchSha256']
        for p in ['SOURCE_BASELINE.json','upstream/source-inventory.json','upstream/LICENSE','upstream/NOTICE']:
            previous[p]=sha((root/p).read_bytes())
    attributes=(root/'.gitattributes').read_bytes() if (root/'.gitattributes').exists() else b''
    if (root/'.gitattributes').exists() and '.gitattributes' not in previous:previous['.gitattributes']=sha(attributes)
    if attributes.count(ATTR_BEGIN)!=attributes.count(ATTR_END) or attributes.count(ATTR_BEGIN)>1:raise ValueError('Unexpected managed attribute blocks')
    if ATTR_BEGIN in attributes:
        before,managed=attributes.split(ATTR_BEGIN,1)
        _,after=managed.split(ATTR_END,1);attributes=before+after
    changed=set(git(source,'diff','--name-only',OFFICIAL,PATCHED,'--',*PREFIXES).decode().splitlines())
    patch=git(source,'diff','--binary','--no-ext-diff','--no-textconv',OFFICIAL,PATCHED,'--',*PREFIXES)
    tree={}
    for row in git(source,'ls-tree','-r','-z',PATCHED,*PREFIXES).split(b'\0'):
        if row:
            metadata,path=row.split(b'\t');tree[path.decode()]=metadata.decode().split()[2]
    archive=git(source,'archive','--format=tar',PATCHED,*PREFIXES)
    inventory=[];excluded=[];symlinks=[];outputs={}
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        for member in stream:
            path=member.name
            if member.isdir():continue
            if '/.github/' in path or path.startswith('.github/'):
                excluded.append({'path':path,'reason':'upstream automation is not enabled by this source import'});continue
            if any(p in path.split('/') for p in ['.git','target','node_modules','checks']) or path.lower().endswith(('.exe','.dll','.asar','.pdb','.sqlite','.sqlite3','.db','.log')):raise ValueError(f'Excluded binary/runtime path in public source selection: {path}')
            if member.issym():
                if path!='codex-rs/vendor/bubblewrap/LICENSE' or member.linkname!='COPYING':raise ValueError('Unexpected source symlink')
                raw=member.linkname.encode();symlinks.append({'path':path,'target':member.linkname})
            elif member.isfile():raw=stream.extractfile(member).read()
            else:raise ValueError('Unexpected archive member type')
            if hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()!=tree[path]:raise ValueError(f'Archive bytes differ from Git blob: {path}')
            if path in WINDOWS_INPUTS:
                item=WINDOWS_INPUTS[path]
                if sha(raw)!=item['gitBlobSha256'] or sha(raw.replace(b'\n',b'\r\n'))!=item['windowsSourceSha256']:raise ValueError(f'Pinned prompt input differs: {path}')
            header=notice(path) if path in changed else b''
            exported=header+raw
            merged={}
            if path=='.gitattributes':
                if attributes and not attributes.endswith(b'\n'):raise ValueError('Existing attributes need a final newline')
                exported=attributes+ATTR_BEGIN+header+raw+prompt_attributes()+ATTR_END
                merged={'attributesMerged':True,'repositoryPrefixBytes':len(attributes),'repositoryPrefixSha256':sha(attributes)}
            outputs[path]=exported
            inventory.append({'path':path,'gitMode':'120000' if member.issym() else ('100755' if member.mode&0o111 else '100644'),'gitBlob':tree[path],'sourceSha256':sha(raw),'publishedSha256':sha(exported),'bytes':len(exported),'modified':path in changed,'noticeOnly':bool(header) and not merged,**merged})
    for name in ['LICENSE','NOTICE']:outputs['upstream/'+name]=git(source,'show',PATCHED+':'+name)
    outputs[PATCH_PATH]=patch
    outputs['upstream/source-inventory.json']=encode({'schema':2,'sourceCommit':PATCHED,'files':inventory,'excluded':excluded,'symlinks':symlinks})
    outputs['SOURCE_BASELINE.json']=encode({'schema':2,'upstream':'https://github.com/openai/codex','officialVersion':'0.166.0','previousPublicCommit':PREVIOUS_PUBLIC,'officialCommit':OFFICIAL,'patchedCommit':PATCHED,'officialTree':git(source,'rev-parse',OFFICIAL+'^{tree}').decode().strip(),'patchedTree':git(source,'rev-parse',PATCHED+'^{tree}').decode().strip(),'selectedPrefixes':PREFIXES,'patchPath':PATCH_PATH,'patchSha256':sha(patch),'sourceInventorySha256':sha(outputs['upstream/source-inventory.json']),'windowsPromptInputsPath':WINDOWS_INPUTS_PATH,'windowsPromptInputsSha256':sha((root/WINDOWS_INPUTS_PATH).read_bytes()),'sourceFileCount':len(inventory),'publishedBytes':sum(p['bytes'] for p in inventory),'modifiedNoticeDate':DATE,'modifiedNoticeLabel':LABEL,'modifiedFiles':[p for p in inventory if p['modified']],'preservedRepositoryDocuments':existing,'limitations':['This public export has not been built or run as a fresh public checkout.','Operational checks, logs, credentials, binaries, local receipts and local Git history are excluded.','Tracked compressed schemas and other upstream source resources remain build inputs, not installable binaries.','Cargo.lock and workspace path dependencies are preserved; no machine-local cache/config overrides are exported.']})
    publish(root,outputs,previous)
    if os.name=='nt':
        for path,item in WINDOWS_INPUTS.items():
            target=checked_target(root,path);data=outputs[path].replace(b'\n',b'\r\n')
            if sha(data)!=item['windowsSourceSha256']:raise ValueError(f'Windows prompt transform differs: {path}')
            if target.read_bytes()!=data:target.write_bytes(data)
    assert existing=={p:sha((root/p).read_bytes()) for p in PROTECTED}
    print(json.dumps({'status':'PASS','sourceFiles':len(inventory),'modifiedFiles':sum(p['modified'] for p in inventory),'excludedAutomationFiles':len(excluded),'symlinks':symlinks,'patchSha256':sha(patch)}))
if __name__=='__main__':main()
