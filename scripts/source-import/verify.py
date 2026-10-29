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
def generated_ts_header(root):
    source=(root/'codex-rs/app-server-protocol/src/export.rs').read_text(encoding='utf-8')
    match=re.search(r'const GENERATED_TS_HEADER: &str = ("(?:[^"\\]|\\.)*");',source)
    if match is None:raise ValueError('Missing actual Rust GENERATED_TS_HEADER contract')
    return json.loads(match.group(1)).encode()

def check_generated_ts_prefix(relative,data,header):
    if relative.startswith('codex-rs/app-server-protocol/schema/typescript/') and relative.endswith('.ts'):
        assert data.replace(b'\r\n',b'\n').startswith(header),f'Generated TypeScript prefix differs: {relative}'

def main():
    baseline=json.loads((ROOT/'SOURCE_BASELINE.json').read_bytes())
    inventory_raw=(ROOT/'upstream/source-inventory.json').read_bytes()
    assert sha(inventory_raw)==baseline['sourceInventorySha256'],'inventory changed'
    inventory=json.loads(inventory_raw)
    assert {item['path'] for item in inventory['excluded']}=={'codex-rs/vendor/bubblewrap/.github/workflows/check.yml'}
    assert not (ROOT/'codex-rs/vendor/bubblewrap/.github/workflows/check.yml').exists()
    from export import WINDOWS_INPUTS,WINDOWS_INPUTS_PATH,canonical_bytes
    assert sha((ROOT/WINDOWS_INPUTS_PATH).read_bytes())==baseline['windowsPromptInputsSha256'],'Windows prompt catalog changed'
    assert baseline['windowsPromptInputsPath']==WINDOWS_INPUTS_PATH
    assert json.loads((ROOT/WINDOWS_INPUTS_PATH).read_bytes())['sourceCommit']==baseline['patchedCommit']
    assert len(WINDOWS_INPUTS)==38
    windows_prompts=0
    ts_header=generated_ts_header(ROOT)
    assert sha((ROOT/baseline['patchPath']).read_bytes())==baseline['patchSha256'],'patch changed'
    changed={item['path'] for item in baseline['modifiedFiles']}
    assert baseline['officialCommit']=='360319d8dee62d87e2d59fc0c21805d79519cfed'
    assert baseline['patchedCommit']=='3a03cbdadf186d74af73fcc5825afc25d3d85f39'
    assert inventory['sourceCommit']==baseline['patchedCommit']
    assert len(inventory['files'])==baseline['sourceFileCount']
    assert len(changed)==357
    includes=0;manifests=0;path_inputs=0;members=0
    for item in inventory['files']:
        relative=item['path'];path=ROOT/relative
        data=read_source(ROOT,item)
        if relative in WINDOWS_INPUTS:
            if os.name=='nt':assert sha(data)==WINDOWS_INPUTS[relative]['windowsSourceSha256'],f'Windows prompt EOL differs: {relative}'
            windows_prompts+=1
        data=canonical_bytes(relative,data)
        check_generated_ts_prefix(relative,data,ts_header)
        assert len(data)==item['bytes'] and sha(data)==item['publishedSha256'],relative
        if item.get('attributesMerged'):
            from export import notice,ATTR_BEGIN,ATTR_END,prompt_attributes
            prefix=item['repositoryPrefixBytes']
            assert sha(data[:prefix])==item['repositoryPrefixSha256'],relative
            header=ATTR_BEGIN+notice(relative)
            assert relative=='.gitattributes' and data[prefix:].startswith(header) and data.endswith(prompt_attributes()+ATTR_END),relative
            raw=data[prefix+len(header):-len(prompt_attributes()+ATTR_END)]
        elif item['noticeOnly']:
            from export import notice
            header=notice(relative)
            assert header and data.startswith(header),relative
            raw=data[len(header):]
        else:raw=data
        assert sha(raw)==item['sourceSha256'],relative
        assert hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==item['gitBlob'],relative
        assert '/.github/' not in relative and not relative.startswith('.github/'),relative
        assert not any(x in relative.split('/') for x in ['.git','target','node_modules','checks']),relative
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
    assert windows_prompts==38
    for relative,expected in baseline['preservedRepositoryDocuments'].items():assert sha((ROOT/relative).read_bytes())==expected,relative
    for name in ['bubblewrap.c','bind-mount.c','network.c','utils.c','COPYING']:assert (ROOT/'codex-rs/vendor/bubblewrap'/name).is_file(),name
    assert read_source(ROOT,{'path':LICENSE_LINK,'gitMode':'120000'})==b'COPYING'
    assert (ROOT/'codex-rs/windows-sandbox-rs/codex-windows-sandbox-setup.manifest').is_file()
    lock=tomllib.loads((ROOT/'codex-rs/Cargo.lock').read_text())
    mio=[p for p in lock['package'] if p['name']=='mio']
    assert len(mio)==1 and mio[0]['version']=='1.2.4' and mio[0]['source'].startswith('registry+'),'mio source'
    print(json.dumps({'status':'PASS','sourceFiles':len(inventory['files']),'modifiedFiles':len(changed),'cargoManifests':manifests,'workspaceMembers':members,'cargoPathInputs':path_inputs,'literalRustIncludes':includes,'lockPackages':len(lock['package']),'mio':'registry 1.2.4','preservedRootDocuments':len(baseline['preservedRepositoryDocuments']),'embeddedPromptInputs':windows_prompts,'windowsPromptRawHashes':'PASS' if os.name=='nt' else 'NOT_RUN: non-Windows host','build':'NOT_RUN','runtime':'NOT_RUN'}))
if __name__=='__main__':main()
