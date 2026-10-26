"""Offline source provenance and Cargo input checks; does not compile or launch code."""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib

if sys.flags.optimize!=0:
    raise RuntimeError('Run source verification without Python optimization; assertion checks are required')

ROOT=Path(__file__).resolve().parents[2]
LICENSE_LINK='codex-rs/vendor/bubblewrap/LICENSE'
def sha(data):return hashlib.sha256(data).hexdigest()
def read_source(root,item):
    relative=Path(item['path'])
    if relative.is_absolute() or '..' in relative.parts:raise ValueError('Unsafe inventory path')
    root=root.resolve();path=root/relative
    if path.parent.resolve()!=root.joinpath(*relative.parts[:-1]):raise ValueError('Unexpected parent symlink or escape')
    if item['gitMode']=='120000':
        if relative.as_posix()!=LICENSE_LINK:raise ValueError('Unexpected source symlink')
        value=os.readlink(path).encode() if path.is_symlink() else path.read_bytes()
        if value!=b'COPYING':raise ValueError('Unexpected license symlink target')
        target=path.parent/'COPYING'
        if target.is_symlink() or not target.is_file():raise ValueError('Invalid license target')
        return value
    if path.is_symlink() or not path.resolve().is_relative_to(root):raise ValueError('Unexpected source symlink or escape')
    return path.read_bytes()
def main():
    baseline=json.loads((ROOT/'SOURCE_BASELINE.json').read_bytes())
    inventory_raw=(ROOT/'upstream/source-inventory.json').read_bytes()
    assert sha(inventory_raw)==baseline['sourceInventorySha256'],'inventory changed'
    inventory=json.loads(inventory_raw)
    assert sha((ROOT/'upstream/r22.patch').read_bytes())==baseline['patchSha256'],'patch changed'
    changed={item['path'] for item in baseline['r22ModifiedFiles']}
    assert len(changed)==23
    includes=0;manifests=0;path_inputs=0;members=0
    for item in inventory['files']:
        relative=item['path'];path=ROOT/relative
        data=read_source(ROOT,item)
        assert len(data)==item['bytes'] and sha(data)==item['publishedSha256'],relative
        if item['noticeOnly']:
            prefix='#' if relative.endswith(('Cargo.toml','Cargo.lock')) else '//'
            header=f'{prefix} Modified for OpenDIY Codex r22 ({baseline["modifiedNoticeDate"]}); see MODIFICATIONS.md.\n'.encode()
            assert data.startswith(header),relative
            raw=data[len(header):]
            built=raw if item['buildInputLineEndings']=='blob-exact' else raw.replace(b'\n',b'\r\n')
            assert sha(built)==item['builtSourceSha256'] and len(built)==item['builtSourceBytes'],relative
        else:raw=data
        assert sha(raw)==item['sourceSha256'],relative
        assert hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==item['gitBlob'],relative
        assert '/.github/' not in relative and not relative.startswith('.github/'),relative
        assert not any(x in relative.split('/') for x in ['.git','target','node_modules']),relative
        assert path.name.lower() not in ['auth.json','.env'],relative
        assert path.suffix.lower() not in ['.exe','.dll','.asar','.sqlite','.sqlite3','.db','.log'],relative
        if path.name=='Cargo.toml':
            doc=tomllib.loads(data.decode());manifests+=1
            if relative=='codex-rs/Cargo.toml':
                for member in doc['workspace']['members']:
                    assert (path.parent/member/'Cargo.toml').is_file(),member
                    members+=1
            def walk(value):
                nonlocal path_inputs
                if isinstance(value,dict):
                    for key,child in value.items():
                        if key=='path' and isinstance(child,str):
                            target=(path.parent/child).resolve();assert target.is_relative_to(ROOT/'codex-rs') and target.exists(),(relative,child);path_inputs+=1
                        else:walk(child)
                elif isinstance(value,list):
                    for child in value:walk(child)
            walk(doc)
        if path.suffix=='.rs':
            for match in re.finditer(r'\binclude(?:_str|_bytes)?!\(\s*"([^"\n]+)"\s*\)',data.decode()):
                target=match.group(1)
                if '$' not in target:
                    assert (path.parent/target).is_file(),(relative,target)
                    includes+=1
    for relative,expected in baseline['preservedRepositoryDocuments'].items():assert sha((ROOT/relative).read_bytes())==expected,relative
    for name in ['bubblewrap.c','bind-mount.c','network.c','utils.c','COPYING']:assert (ROOT/'codex-rs/vendor/bubblewrap'/name).is_file(),name
    assert read_source(ROOT,{'path':LICENSE_LINK,'gitMode':'120000'})==b'COPYING'
    assert (ROOT/'codex-rs/windows-sandbox-rs/codex-windows-sandbox-setup.manifest').is_file()
    lock=tomllib.loads((ROOT/'codex-rs/Cargo.lock').read_text())
    mio=[p for p in lock['package'] if p['name']=='mio']
    assert len(mio)==1 and mio[0]['version']=='1.2.4' and mio[0]['source'].startswith('registry+'),'mio source'
    print(json.dumps({'status':'PASS','sourceFiles':len(inventory['files']),'r22ModifiedFiles':len(changed),'cargoManifests':manifests,'workspaceMembers':members,'cargoPathInputs':path_inputs,'literalRustIncludes':includes,'lockPackages':len(lock['package']),'mio':'registry 1.2.4','preservedRootDocuments':3,'build':'NOT_RUN','runtime':'NOT_RUN'}))
if __name__=='__main__':main()
