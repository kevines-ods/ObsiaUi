import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
export default function ControlPanel(){
  const [thoughts, setThoughts]=useState<string[]>([]);
  const [tools, setTools]=useState<string[]>([]);
  useEffect(()=>{
    const u:(()=>void)[]=[];
    listen<string>("agent:thought", e=>setThoughts(p=>[...p,e.payload])).then(f=>u.push(f));
    listen<string>("agent:tool", e=>setTools(p=>[...p,e.payload])).then(f=>u.push(f));
    return()=>u.forEach(fn=>fn());
  },[]);
  return <div className="control-panel"><h3>Control</h3><section><h4>Thoughts</h4>{thoughts.map((t,i)=><pre key={i}>{t}</pre>)}</section><section><h4>Tool calls</h4>{tools.map((t,i)=><pre key={i}>{t}</pre>)}</section></div>;
}
