import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
type FileEntry={name:string; path:string; isDir:boolean};
export default function FileManager(){
  const [files, setFiles]=useState<FileEntry[]>([]);
  const [path, setPath]=useState("mémoire");
  const [backlinks, setBacklinks]=useState<string[]>([]);
  useEffect(()=>{ invoke<FileEntry[]>("list_vault",{path}).then(setFiles).catch(()=>setFiles([{name:"2026-08-27-lancement-coffre-obsi.md",path:"mémoire/agent 1/projets 1/2026-08-27-lancement-coffre-obsi.md",isDir:false}])); },[path]);
  const open=(f:FileEntry)=>{ if(f.isDir) setPath(f.path); else invoke<string[]>("get_backlinks",{path:f.path}).then(setBacklinks).catch(()=>{}); };
  return <div className="file-manager"><h3>Vault</h3><div className="path">{path} <button onClick={()=>setPath("mémoire")}>root</button></div><ul>{files.map(f=><li key={f.path} onClick={()=>open(f)}>{f.isDir?"📁 ":"📄 "}{f.name}</li>)}</ul>{backlinks.length>0&&<div className="backlinks"><h4>Backlinks</h4><ul>{backlinks.map(b=><li key={b}>{b}</li>)}</ul></div>}</div>;
}
