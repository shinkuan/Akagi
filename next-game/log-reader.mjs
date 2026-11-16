import fs from 'node:fs/promises';
import path from 'node:path';
import {parseEvent} from './core.mjs';
export class LogReader {
  constructor(appDir,logDir){this.root=logDir??path.join(appDir,'logs');this.file=null;this.offset=0;this.partial='';}
  async read() {
    const dirs=(await fs.readdir(this.root,{withFileTypes:true})).filter(d=>d.isDirectory()&&/^\d{8}-\d{6}$/.test(d.name)).map(d=>d.name).sort();
    if(!dirs.length)return {events:[],changed:false};
    const file=path.join(this.root,dirs.at(-1),'all.jsonl');
    const changed=this.file!==null&&this.file!==file;
    if(this.file!==file){this.file=file;this.offset=0;this.partial='';}
    const stat=await fs.stat(file);
    if(stat.size<this.offset){this.offset=0;this.partial='';}
    const handle=await fs.open(file,'r');
    const events=[];
    try {
      while(this.offset<stat.size){
        const buffer=Buffer.alloc(Math.min(1024*1024,stat.size-this.offset));
        const {bytesRead}=await handle.read(buffer,0,buffer.length,this.offset);if(!bytesRead)break;
        this.offset+=bytesRead;
        // Protocol names and event payload IDs are ASCII; split log lines at
        // byte boundaries so UTF-8 names crossing a chunk are never damaged.
        this.partial+=buffer.subarray(0,bytesRead).toString('latin1');
        const lines=this.partial.split('\n');this.partial=lines.pop();
        for(const line of lines){const e=parseEvent(Buffer.from(line,'latin1').toString('utf8'));if(e)events.push(e);}
      }
    } finally {await handle.close();}
    return {events,changed};
  }
}
