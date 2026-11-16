import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';

test('embedded worker serves authenticated IPC only and exits without its parent',async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'akagi-embedded-test-'));
  const listener=net.createServer();listener.listen(0,'127.0.0.1');await once(listener,'listening');
  const port=listener.address().port;await new Promise(r=>listener.close(r));
  const log=path.join(root,'logs','20261117-000000');await fs.mkdir(log,{recursive:true});await fs.writeFile(path.join(log,'all.jsonl'),'');
  const token='test-private-worker-token';
  const child=spawn(process.execPath,[fileURLToPath(new URL('./server.mjs',import.meta.url))],{
    env:{...process.env,AKAGI_APP_DIR:root,AKAGI_LOG_DIR:path.join(root,'logs'),AKAGI_NEXT_PORT:String(port),AKAGI_NEXT_TOKEN:token,AKAGI_PARENT_PID:'99999999'},windowsHide:true,stdio:'pipe',
  });
  let stderr='';child.stderr.on('data',d=>{stderr+=d;});
  const exited=once(child,'exit');
  try{
    const origin=`http://127.0.0.1:${port}`;
    let online=false;
    for(let i=0;i<30;i++){try{online=(await fetch(origin+'/health')).ok;if(online)break;}catch{}await new Promise(r=>setTimeout(r,30));}
    assert.ok(online,stderr);
    assert.equal((await fetch(origin+'/')).status,403);
    assert.equal((await fetch(origin+'/state')).status,403);
    const state=await(await fetch(origin+'/state',{headers:{'X-Akagi-Token':token}})).json();
    assert.equal(state.enabled,false);
    const stopped=await fetch(origin+'/control',{method:'POST',headers:{Origin:origin,'Content-Type':'application/json','X-Akagi-Token':token},body:JSON.stringify({enabled:false})});
    assert.equal(stopped.status,200);
    const timeout=setTimeout(()=>child.kill(),5000);
    const [code]=await exited;clearTimeout(timeout);assert.equal(code,0,stderr);
  }finally{if(child.exitCode===null)child.kill();assert.equal(path.dirname(root),path.resolve(os.tmpdir()));await fs.rm(root,{recursive:true,force:true});}
});
