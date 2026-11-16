export class CDP {
  static async connect(url) {
    const c = new CDP();
    c.ws = new WebSocket(url);
    c.next = 0; c.pending = new Map();
    c.ws.addEventListener('message', ({data}) => {
      const m = JSON.parse(data); const p = c.pending.get(m.id);
      if (p) { c.pending.delete(m.id); clearTimeout(p.timer); m.error ? p.reject(new Error(m.error.message)) : p.resolve(m.result); }
    });
    c.ws.addEventListener('close', () => {
      for (const p of c.pending.values()) { clearTimeout(p.timer); p.reject(new Error('浏览器连接关闭')); }
      c.pending.clear();
    });
    await new Promise((resolve,reject) => {
      const timer = setTimeout(() => { c.ws.close(); reject(new Error('连接超时')); }, 5000);
      c.ws.addEventListener('open', () => {clearTimeout(timer);resolve();}, {once:true});
      c.ws.addEventListener('error', () => {clearTimeout(timer);reject(new Error('连接失败'));}, {once:true});
    });
    return c;
  }
  send(method,params={}) {
    return new Promise((resolve,reject) => {
      const id = ++this.next;
      const timer = setTimeout(() => {this.pending.delete(id); reject(new Error('浏览器响应超时'));},5000);
      this.pending.set(id,{resolve,reject,timer});
      this.ws.send(JSON.stringify({id,method,params}));
    });
  }
  async evaluate(expression) {
    const r = await this.send('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
    if(r.exceptionDetails) throw new Error(r.exceptionDetails.text);
    return r.result.value;
  }
  close(){this.ws.close();}
}
