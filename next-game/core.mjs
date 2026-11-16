export function parseEvent(line) {
  try {
    const {message:m, ts_ms:time} = JSON.parse(line);
    if (typeof m !== 'string') return null;
    const payload = () => JSON.parse(m.slice(m.indexOf('{')));
    if (/up REQUEST .* \.lq\.FastTest\.authGame /.test(m)) {
      const id=payload().game_uuid;
      return typeof id==='string' && id ? {type:'auth',id,time} : null;
    }
    if (/^seat resolved: .*seat=\d/.test(m)) return {type:'ready',time};
    if (/down NOTIFY .* \.lq\.NotifyGameEndResult /.test(m)) return {type:'end',time};
    if (/down NOTIFY .* \.lq\.NotifyGameTerminate /.test(m)) return {type:'terminate',time};
    if (/up REQUEST .* \.lq\.Lobby\.startUnifiedMatch /.test(m)) return {type:'queue',time};
    return null;
  } catch {return null;}
}

export class Session {
  constructor(){this.enabled=false;this.limited=true;this.limit=3;this.remaining=3;this.started=0;this.phase='idle';this.gameId=null;this.inGame=false;this.ended=false;this.reason='自动续局已关闭';this.since=0;this.generation=0;this.resultSteps=0;}
  arm({limited=true,limit=3}, now=Date.now()) {
    if(typeof limited!=='boolean'||!Number.isInteger(limit)||limit<1||limit>99) throw Error('场数必须是 1–99 的整数');
    this.generation++;this.enabled=true;this.limited=limited;this.limit=limit;this.remaining=limit;this.started=0;this.resultSteps=0;this.phase=this.ended?'result':'playing';this.since=now;this.reason=this.ended?'等待结算按钮':'等待当前整场结束';
  }
  stop(reason='自动续局已关闭'){this.generation++;this.enabled=false;this.phase='idle';this.reason=reason;}
  event(e,now=Date.now()) {
    if(e.type==='auth'){this.pendingId=e.id;return;}
    if(e.type==='ready'){if(!this.pendingId)return;e={type:'start',id:this.pendingId};this.pendingId=null;}
    if(e.type==='start' && e.id!==this.gameId) {
      this.gameId=e.id;this.inGame=true;this.ended=false;
      if(this.enabled && this.phase==='queued') {
        this.started++;this.remaining=Math.max(0,this.remaining-1);
        if(this.limited && this.remaining===0){this.stop('最后一场已开始；打完后不再续局');return;}
      }
      if(this.enabled){this.phase='playing';this.reason='对局进行中，等待整场结束';this.since=now;}
    } else if(e.type==='end' && this.inGame) {
      this.inGame=false;this.ended=true;this.resultSteps=0;
      if(this.enabled){this.phase='result';this.reason='整场已结束，等待结算按钮';this.since=now;}
    } else if(e.type==='terminate') {
      this.inGame=false;this.ended=false;if(this.enabled)this.stop('对局异常终止，已停止续局');
    } else if(e.type==='queue' && this.enabled && ['result','settling','confirming'].includes(this.phase)) {
      // A user manually queued while we were observing the result: never click again.
      this.stop('检测到手动匹配，已停止本轮自动续局');
    }
  }
  settling(now=Date.now()){this.resultSteps++;this.phase='settling';this.reason='已确认终局排名，等待再来一场';this.since=now;}
  settling(now=Date.now()){this.phase='settling';this.reason='已确认终局排名，等待再来一场';this.since=now;}
  confirming(now=Date.now()){this.phase='confirming';this.reason='已点击再来一场，等待续局确认框';this.since=now;}
  queued(now=Date.now()){this.phase='queued';this.reason='已确认续局，等待新对局开始';this.since=now;}
  checkTimeout(now=Date.now()) {
    if(this.enabled && ['result','settling','confirming','queued'].includes(this.phase) && now-this.since>180000)
      this.stop(this.phase!=='queued'?'未识别到续局按钮，已停止；请手动处理结算界面':'匹配等待超过 3 分钟，已停止后续续局');
  }
  status(){return {enabled:this.enabled,limited:this.limited,limit:this.limit,remaining:this.remaining,started:this.started,phase:this.phase,inGame:this.inGame,reason:this.reason};}
}

// Only explicit rematch buttons qualify. Never click a generic confirmation,
// rank-room selection, purchase, or a hand-ending button.
export function findRematch(ocr) {
  const allowed=new Set(['再来一局','再來一局','再来一场','再來一場','再次匹配','繼續匹配','继续匹配']);
  const hits=[];
  for(const line of ocr.lines??[]) {
    if(!allowed.has(line.text.replace(/\s/g,'')))continue;
    const words=line.words??[];if(!words.length)continue;
    const left=Math.min(...words.map(w=>w.x)),top=Math.min(...words.map(w=>w.y));
    const right=Math.max(...words.map(w=>w.x+w.width)),bottom=Math.max(...words.map(w=>w.y+w.height));
    const x=(left+right)/2,y=(top+bottom)/2;
    if(x>0&&x<ocr.width&&y>ocr.height*.4&&y<ocr.height)hits.push({x,y,width:ocr.width,height:ocr.height,label:line.text.replace(/\s/g,'')});
  }
  return hits.length===1?hits[0]:null;
}

export function findConfirmation(ocr,verifiedButton=null){
  const text=(ocr.lines??[]).map(l=>l.text.replace(/\s/g,''));
  if(!text.some(t=>/^再[來来]一[場场]$/.test(t))||!text.some(t=>/^要再[進进]行一[場场]$/.test(t))||!text.some(t=>/^的匹配[對对]局[嗎吗][?？]$/.test(t)))return null;
  const matches=(ocr.lines??[]).filter(l=>/^(確認|确认)$/.test(l.text.replace(/\s/g,'')));
  if(matches.length===0&&verifiedButton?.x!==undefined)return {...verifiedButton,kind:'confirm',label:'确认续局'};
  if(matches.length!==1)return null;
  const words=matches[0].words;if(!words?.length)return null;
  const x=(Math.min(...words.map(w=>w.x))+Math.max(...words.map(w=>w.x+w.width)))/2;
  const y=(Math.min(...words.map(w=>w.y))+Math.max(...words.map(w=>w.y+w.height)))/2;
  if(x<ocr.width*.25||x>ocr.width*.65||y<ocr.height*.5||y>ocr.height*.85)return null;
  return {x,y,width:ocr.width,height:ocr.height,label:'确认续局',kind:'confirm'};
}
