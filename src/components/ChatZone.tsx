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
    setMessages(m=>[...m, {role:"user", content: input}]);
    const prompt = input; setInput("");
    await send(prompt, "ollama", "llama3.2").catch(()=>{});
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
