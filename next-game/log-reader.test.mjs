import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {LogReader} from './log-reader.mjs';
test('incremental reader ignores partial lines, never replays, detects rotation',async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'akagi-next-test-'));
  try{
    const d=path.join(root,'logs','20261117-010000');await fs.mkdir(d,{recursive:true});
    const file=path.join(d,'all.jsonl');
    const row=JSON.stringify({ts_ms:1,message:'up REQUEST #1 .lq.FastTest.authGame {"game_uuid":"a","test":"中文"}'})+'\n';
    await fs.writeFile(file,row.slice(0,30));const r=new LogReader(root);
    assert.equal((await r.read()).events.length,0);
    await fs.appendFile(file,row.slice(30));assert.equal((await r.read()).events[0].id,'a');
    assert.equal((await r.read()).events.length,0);
    const d2=path.join(root,'logs','20261117-020000');await fs.mkdir(d2);await fs.writeFile(path.join(d2,'all.jsonl'),row);
    const next=await r.read();assert.equal(next.changed,true);assert.equal(next.events.length,1);
  }finally{assert.equal(path.dirname(path.resolve(root)),path.resolve(os.tmpdir()));assert.ok(path.basename(root).startsWith('akagi-next-test-'));await fs.rm(root,{recursive:true,force:true});}
});
