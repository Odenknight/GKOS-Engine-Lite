import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createServer} from 'node:net';
import {allSnippets} from '../dist-test/snippets.js';
import {defaultSettings} from '../dist-test/settings-schema.js';

test('quick-connect commands match the exact packaged v1.1.3 REST sidecar', {skip:process.platform!=='win32',timeout:30000},async()=>{
 const binary=fileURLToPath(new URL('../src-tauri/binaries/kosmos-agent-x86_64-pc-windows-msvc.exe',import.meta.url));
 assert.equal(createHash('sha256').update(await readFile(binary)).digest('hex'),'29ab43c9ce79b8c14978594a18523a04e3f7518d87560f1e4c17744da08f12c2');
 const folder=await mkdtemp(join(tmpdir(),'lite-rest-fixture-')),notes=join(folder,'notes'),status=join(folder,'desktop-agent.status.json');
 await mkdir(notes);await writeFile(join(notes,'Unlabeled.md'),'# Unlabeled synthetic fixture\n\nNo private source data.\n');
 const listener=createServer();await new Promise(r=>listener.listen(0,'127.0.0.1',r));const port=listener.address().port;await new Promise(r=>listener.close(r));
 assert.equal(defaultSettings().enabled,false);assert.equal(defaultSettings().default_sensitivity,'secret');assert.equal(defaultSettings().port,4814);
 const child=spawn(binary,['--notes',notes,'--port',String(port),'--status-file',status],{windowsHide:true,stdio:'ignore'});
 try{
  let token;
  for(let i=0;i<100;i++){
   try{token=(await readFile(join(folder,'desktop-agent.token'),'utf8')).trim();const r=await fetch(`http://127.0.0.1:${port}/health`,{headers:{Authorization:`Bearer ${token}`}});if(r.ok)break;}catch{}
   if(child.exitCode!==null)throw Error('Packaged sidecar exited before readiness');
   await new Promise(r=>setTimeout(r,100));
  }
  assert.ok(token,'sidecar generated a test-only token');
  const origin=`http://127.0.0.1:${port}`,headers={Authorization:`Bearer ${token}`};
  assert.equal((await fetch(origin+'/health')).status,401);
  assert.equal((await fetch(origin+'/mcp',{headers})).status,404);
  assert.equal((await fetch(origin+'/mcp',{method:'POST',headers})).status,405);
  for(const [key,command] of Object.entries(allSnippets({port,token},'windows'))){
   const url=command.match(/http:\/\/127\.0\.0\.1:\d+[^"\s]*/)?.[0];assert.ok(url,key);
   const response=await fetch(url,{headers});assert.equal(response.status,200,`${key} advertises an unavailable packaged route`);
   assert.match(command,/^curl\.exe -H "Authorization: Bearer /);
   const args=[...command.matchAll(/\"([^\"]*)\"|(\S+)/g)].map(match=>match[1]??match[2]);
   const {stdout}=await promisify(execFile)(args[0],args.slice(1),{windowsHide:true});
   const result=JSON.parse(stdout);
   assert.deepEqual(result,await response.json());
   if(key==='notes'){
    assert.ok(JSON.stringify(result).includes('Unlabeled.md'));
    assert.equal(result.notes.find(note=>note.path==='Unlabeled.md').sensitivity,null,'REST projection preserves absent metadata');
   }
  }
  console.log('Packaged REST runtime: four advertised GET routes 200; unauthenticated401; GET/mcp404; POST/mcp405; unlabeled notes readable with token despite default-secret launch setting.');
 }finally{
  if(child.exitCode===null){const ended=new Promise(r=>child.once('exit',r));child.kill();await ended;}
  // Only this test-created absolute temporary subtree is removed.
  assert.ok(resolve(folder).startsWith(resolve(tmpdir())+String.fromCharCode(92)));
  await rm(folder,{recursive:true,force:true});
 }
});
