import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Session,findRematch,findConfirmation,parseEvent} from './core.mjs';
test('terminal result confirmation consumes no quota and times out safely',()=>{
  const s=new Session();s.event({type:'start',id:'current'});s.arm({limit:1},0);s.event({type:'end'},0);s.settling(0);
  assert.equal(s.phase,'settling');assert.equal(s.remaining,1);assert.equal(s.started,0);
  s.checkTimeout(180001);assert.equal(s.enabled,false);
});
test('three extra games, duplicate auth and end events do not consume quota',()=>{
  const s=new Session();s.event({type:'start',id:'current'});s.arm({limit:3});
  for(let i=0;i<3;i++){
    s.event({type:'end'});s.event({type:'end'});assert.equal(s.remaining,3-i);
    s.queued();s.event({type:'start',id:'next'+i});s.event({type:'start',id:'next'+i});
    assert.equal(s.remaining,2-i);
  }
  assert.equal(s.started,3);assert.equal(s.enabled,false);assert.equal(s.inGame,true);
});
test('stop, termination, timeout and manual queue prevent further clicks',()=>{
  for(const kind of ['stop','terminate','timeout','queue']){
    const s=new Session();s.event({type:'start',id:'a'});s.arm({limit:1},0);s.event({type:'end'},0);
    if(kind==='stop')s.stop();else if(kind==='timeout')s.checkTimeout(180001);else s.event({type:kind});
    assert.equal(s.enabled,false,kind);assert.equal(s.started,0);
  }
});
test('unlimited is explicit; invalid limits rejected; initial lobby is not end',()=>{
  const s=new Session();s.arm({limited:false,limit:1});assert.equal(s.phase,'playing');
  s.event({type:'end'});assert.equal(s.phase,'playing');
  s.event({type:'start',id:'a'});s.event({type:'end'});s.queued();s.event({type:'start',id:'b'});
  assert.equal(s.enabled,true);for(const n of [0,100,1.5,NaN])assert.throws(()=>s.arm({limit:n}));
});
test('only complete game end and client auth messages are tracked',()=>{
  const log=message=>JSON.stringify({ts_ms:1,message});
  assert.equal(parseEvent(log('down NOTIFY - .lq.ActionPrototype {"name":"ActionHule"}')),null);
  assert.deepEqual(parseEvent(log('down NOTIFY - .lq.NotifyGameEndResult {}')),{type:'end',time:1});
  assert.equal(parseEvent(log('down RESPONSE #1 .lq.FastTest.authGame {}')),null);
  assert.equal(parseEvent(log('up REQUEST #1 .lq.FastTest.authGame {"game_uuid":"x"}')).id,'x');
});
test('failed authentication never consumes a game; confirmation does, reconnect does not',()=>{
  const s=new Session();s.event({type:'start',id:'a'});s.arm({limit:2});s.event({type:'end'});s.queued();
  s.event({type:'auth',id:'b'});assert.equal(s.remaining,2);
  s.event({type:'ready'});assert.equal(s.remaining,1);
  s.event({type:'auth',id:'b'});s.event({type:'ready'});assert.equal(s.remaining,1);
});
test('OCR accepts unique rematch label only; rejects generic buttons and ambiguity',()=>{
  const line=text=>({text,words:[{x:800,y:700,width:120,height:30}]});
  const o=lines=>({width:1200,height:900,lines});
  assert.equal(findRematch(o([line('再 來 一 局')])).x,860);
  for(const t of ['确认','返回大厅','开始匹配','充值','再来一局购买'])assert.equal(findRematch(o([line(t)])),null);
  assert.equal(findRematch(o([line('再来一局'),line('再来一局')])),null);
});
test('confirmation requires the complete rematch prompt, never a generic dialog',()=>{
  const l=text=>({text,words:[{x:800,y:700,width:120,height:30}]});
  const o={width:2000,height:1000,lines:['再 來 一 場','要 再 進 行 一 場','的 匹 配 對 局 嗎 ？','確 認'].map(l)};
  assert.equal(findConfirmation(o).kind,'confirm');
  assert.equal(findConfirmation({...o,lines:[l('確認')]}),null);
  assert.equal(findConfirmation({...o,lines:o.lines.slice(1)}),null);
  const button={x:860,y:715,width:2000,height:1000};
  assert.equal(findConfirmation({...o,lines:o.lines.slice(0,3)},button).kind,'confirm');
  assert.equal(findConfirmation({...o,lines:[]},button),null);
  const s=new Session();s.event({type:'start',id:'a'});s.arm({limit:1});s.event({type:'end'});s.confirming();assert.equal(s.remaining,1);s.checkTimeout(Date.now()+181000);assert.equal(s.enabled,false);
});
