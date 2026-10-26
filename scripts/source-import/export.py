"""Export pinned public Git blobs, not a machine's working files or Git history."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

OFFICIAL='6905ecede7816e91373e9eabda3f6b0368fc747b'
PATCHED='f74c7574ae39b95550d6b445f828ba1010787f74'
PREFIXES=['codex-rs','third_party','patches','.cargo/config.toml']
PROTECTED=['LICENSE','NOTICE','README.md']
DATE='2026-10-26'
def sha(data):return hashlib.sha256(data).hexdigest()
def git(repository,*args):return subprocess.check_output(['git','-c','core.autocrlf=false','-C',str(repository),*args])
def notice(path):
    prefix='#' if path.endswith(('Cargo.toml','Cargo.lock')) else '//'
    return f'{prefix} Modified for OpenDIY Codex r22 ({DATE}); see MODIFICATIONS.md.\n'.encode()
def write(root,path,data):
    target=root/path
    if not target.resolve().is_relative_to(root.resolve()) or target.is_symlink():raise ValueError('Unsafe export target')
    if target.exists():
        if target.read_bytes()!=data:raise ValueError(f'Existing file differs; refusing overwrite: {path}')
    else:
        target.parent.mkdir(parents=True,exist_ok=True)
        with target.open('xb') as stream:stream.write(data)
def encode(value):return (json.dumps(value,ensure_ascii=False,indent=2)+'\n').encode()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-repository',type=Path)
    parser.add_argument('--output',type=Path,default=Path(__file__).resolve().parents[2])
    parser.add_argument('--stage-modes',action='store_true',help='Restore imported Git modes after git add; does not change file bytes')
    args=parser.parse_args();root=args.output.resolve();source=args.source_repository
    if args.stage_modes:
        inventory=json.loads((root/'upstream/source-inventory.json').read_bytes())
        staged={}
        for row in git(root,'ls-files','--stage','-z').split(b'\0'):
            if row:
                metadata,path=row.split(b'\t');staged[path.decode()]=metadata.decode().split()[1]
        rows=[]
        for item in inventory['files']:
            path=item['path'];data=(root/path).read_bytes()
            if sha(data)!=item['publishedSha256'] or path not in staged:raise ValueError(f'Source not staged as reviewed: {path}')
            rows.append(f'{item["gitMode"]} {staged[path]}\t{path}\n')
        subprocess.run(['git','-C',str(root),'update-index','--index-info'],input=''.join(rows).encode(),check=True)
        print(json.dumps({'status':'PASS','stagedSourceModes':len(rows)}));return
    if source is None:parser.error('--source-repository is required for source export')
    if git(source,'rev-parse',OFFICIAL).decode().strip()!=OFFICIAL or git(source,'rev-parse',PATCHED).decode().strip()!=PATCHED:raise ValueError('Missing source commits')
    existing={p:sha((root/p).read_bytes()) for p in PROTECTED}
    changed=git(source,'diff','--name-only',OFFICIAL,PATCHED).decode().splitlines()
    if len(changed)!=23 or any(not p.startswith('codex-rs/') for p in changed):raise ValueError('Unexpected r22 patch scope')
    patch=git(source,'diff','--binary','--no-ext-diff','--no-textconv',OFFICIAL,PATCHED)
    tree={}
    for row in git(source,'ls-tree','-r','-z',PATCHED,*PREFIXES).split(b'\0'):
        if row:
            metadata,path=row.split(b'\t');tree[path.decode()]=metadata.decode().split()[2]
    build_catalog=json.loads((root/'upstream/r22-build-input-hashes.json').read_bytes())
    build_inputs={item['path']:item for item in build_catalog['files']}
    archive=git(source,'archive','--format=tar',PATCHED,*PREFIXES)
    inventory=[];excluded=[];symlinks=[]
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        for member in stream:
            path=member.name
            if member.isdir():continue
            if '/.github/' in path or path.startswith('.github/'):
                excluded.append({'path':path,'reason':'upstream automation is not enabled by this source import'});continue
            if any(p in path.split('/') for p in ['.git','target','node_modules']) or path.lower().endswith(('.exe','.dll','.asar','.sqlite','.db')):raise ValueError(f'Excluded binary/runtime path in public source selection: {path}')
            if member.issym():
                if path!='codex-rs/vendor/bubblewrap/LICENSE' or member.linkname!='COPYING':raise ValueError('Unexpected source symlink')
                raw=member.linkname.encode();symlinks.append({'path':path,'target':member.linkname})
            elif member.isfile():raw=stream.extractfile(member).read()
            else:raise ValueError('Unexpected archive member type')
            if hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()!=tree[path]:raise ValueError(f'Archive bytes differ from Git blob: {path}')
            exported=notice(path)+raw if path in changed else raw
            write(root,path,exported)
            item={'path':path,'gitMode':'120000' if member.issym() else ('100755' if member.mode&0o111 else '100644'),'gitBlob':tree[path],'sourceSha256':sha(raw),'publishedSha256':sha(exported),'bytes':len(exported),'noticeOnly':path in changed}
            if path in changed:
                built=build_inputs[path];reconstructed=raw if sha(raw)==built['sha256'] else raw.replace(b'\n',b'\r\n')
                if sha(reconstructed)!=built['sha256'] or len(reconstructed)!=built['bytes']:raise ValueError('Frozen r22 build-input mismatch')
                item.update(builtSourceSha256=built['sha256'],builtSourceBytes=built['bytes'],buildInputLineEndings='blob-exact' if reconstructed==raw else 'CRLF')
            inventory.append(item)
    for name in ['LICENSE','NOTICE']:
        data=git(source,'show',PATCHED+':'+name);write(root,'upstream/'+name,data)
    write(root,'upstream/r22.patch',patch)
    write(root,'upstream/source-inventory.json',encode({'schema':1,'sourceCommit':PATCHED,'files':inventory,'excluded':excluded,'symlinks':symlinks}))
    by_path={item['path']:item for item in inventory}
    baseline={'schema':1,'upstream':'https://github.com/openai/codex','officialCommit':OFFICIAL,'patchedCommit':PATCHED,'officialTree':git(source,'rev-parse',OFFICIAL+'^{tree}').decode().strip(),'patchedTree':git(source,'rev-parse',PATCHED+'^{tree}').decode().strip(),'patchSha256':sha(patch),'sourceInventorySha256':sha((root/'upstream/source-inventory.json').read_bytes()),'sourceFileCount':len(inventory),'publishedBytes':sum(p['bytes'] for p in inventory),'modifiedNoticeDate':DATE,'r22ModifiedFiles':[by_path[p] for p in changed],'preservedRepositoryDocuments':existing,'limitations':['Full runtime tests and release builds were not run for this public import.','r23 working-tree changes are excluded.','The Cargo build graph remains pinned by Cargo.lock; no local cache/config or private path overrides were exported.']}
    write(root,'SOURCE_BASELINE.json',encode(baseline))
    assert existing=={p:sha((root/p).read_bytes()) for p in PROTECTED}
    print(json.dumps({'status':'PASS','sourceFiles':len(inventory),'r22ModifiedFiles':len(changed),'excludedAutomationFiles':len(excluded),'symlinks':symlinks,'patchSha256':sha(patch)}))
if __name__=='__main__':main()
