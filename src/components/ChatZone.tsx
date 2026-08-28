import { useState, useEffect } from "react";
import { useLlmStream } from "../hooks/useLlmStream";

type Msg = { role: "user"|"assistant"; content: string };

export default function ChatZone() {
  const { tokens, done, error, send } = useLlmStream();
  const [messages, setMessages] = useState<Msg[]>([]);
  const [input, setInput] = useState("");

  useEffect(()=>{ if(done && tokens) setMessages(m=>[...m, {role:"assistant", content: tokens}]); }, [done]);

  const onSend = async () => {
    if(!input.trim()) return;
    const userMsg: Msg = {role:"user", content: input};
    const history = [...messages, userMsg];
    setMessages(history);
    setInput("");
    await send(
      history.map(m=>({role: m.role, content: m.content})),
      "ollama",
      "qwen3.5:0.8b"
    ).catch(()=>{});
  };

  return (
    <div className="chat-zone">
      <div className="messages">
        {messages.map((m,i)=><div key={i} className={`msg ${m.role}`}>{m.content}</div>)}
        {tokens && !done && <div className="msg assistant streaming">{tokens}▌</div>}
        {error && <div className="error">{error}</div>}
      </div>
      <div className="input-row">
        <input value={input} onChange={e=>setInput(e.target.value)} onKeyDown={e=>e.key==="Enter"&&onSend()} placeholder="Ask..." />
        <button onClick={onSend}>Send</button>
      </div>
    </div>
  );
}
