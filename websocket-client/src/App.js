import React, { useState, useRef, useEffect } from "react";
import "./App.css";
import Prism from "prismjs";
import "prismjs/themes/prism-okaidia.css";
import "prismjs/components/prism-rust";

function App() {
  const [socket, setSocket] = useState(null);
  const [inputMessage, setInputMessage] = useState("");
  const [messages, setMessages] = useState([]);

  const [isTyping, setIsTyping] = useState(false);

  // Reference to the chat container for auto-scrolling
  const chatContainerRef = useRef(null);

  const connectWebSocket = () => {
    if (socket) {
      socket.close();
    }

    // Create the WebSocket
    const ws = new WebSocket("ws://127.0.0.1:3456/ws");

    ws.onopen = () => {
      console.log("WebSocket is open now");
      // Optionally, add a "connected" message
      setMessages((prev) => [
        ...prev,
        { text: "Connected to the server!", sender: "server" },
      ]);
    };

    ws.onmessage = (event) => {
      const chunk = event.data;
      console.log("Received from server:", event.data);
      setIsTyping(false);

      const processMessage = (text) => {
        const parts = text.split(/(```[\s\S]*?```)/g);
        return parts.map((part, index) => {
          if (part.startsWith("```")) {
            const languageMatch = part.match(/^```(\w+)/);
            const language = languageMatch ? languageMatch[1] : "";
            const code = part.replace(/```[\s\S]*?\n/, "").replace(/```$/, "");
            return (
              <pre key={index} className={`language-${language}`}>
                <code className={`language-${language}`}>{code.trim()}</code>
              </pre>
            );
          }
          return <span key={index}>{part}</span>;
        });
      };

      setMessages((prev) => {
        const lastMessage = prev.length > 0 ? prev[prev.length - 1] : null;
        if (
          lastMessage &&
          lastMessage.sender === "server" &&
          !lastMessage.complete
        ) {
          // Append to existing message
          const updatedMessage = {
            ...lastMessage,
            text: lastMessage.text + chunk,
            components: processMessage(lastMessage.text + chunk),
          };

          // Check if this is the final chunk (you'll need to implement this)
          if (chunk.endsWith("[DONE]")) {
            // Example end marker
            updatedMessage.complete = true;
            setIsTyping(false);
          }

          return [...prev.slice(0, -1), updatedMessage];
        } else {
          // Start new message
          return [
            ...prev,
            {
              text: chunk,
              sender: "server",
              components: processMessage(chunk),
              complete: false,
            },
          ];
        }
      });
    };

    ws.onclose = () => {
      console.log("WebSocket is closed now");
      setMessages((prev) => [
        ...prev,
        { text: "WebSocket connection closed", sender: "server" },
      ]);
    };

    ws.onerror = (err) => {
      console.error("WebSocket error:", err);
      setMessages((prev) => [
        ...prev,
        { text: `Error: ${err.message}`, sender: "server" },
      ]);
    };

    setSocket(ws);
  };

  const processMessage = (text) => {
    const parts = text.split(/(```[\s\S]*?```)/g);
    return parts.map((part, index) => {
      if (part.startsWith("```")) {
        const languageMatch = part.match(/^```(\w+)/);
        const language = languageMatch ? languageMatch[1] : "";
        const code = part.replace(/```[\s\S]*?\n/, "").replace(/```$/, "");
        return (
          <pre key={index} className={`language-${language}`}>
            <code className={`language-${language}`}>{code.trim()}</code>
          </pre>
        );
      }
      return <span key={index}>{part}</span>;
    });
  };

  const sendMessage = () => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      // Process user's message for code blocks
      const processedComponents = processMessage(inputMessage);

      // Add processed message to state
      setMessages((prev) => [
        ...prev,
        {
          text: inputMessage,
          sender: "me",
          components: processedComponents,
        },
      ]);

      // Send raw text to server
      socket.send(inputMessage);
      setInputMessage("");
      setIsTyping(true);
    }
  };

  // Optionally send message on Enter key
  const handleKeyDown = (e) => {
    if (e.key === "Enter") {
      sendMessage();
    }
  };

  useEffect(() => {
    Prism.highlightAll();
  }, [messages]);

  // Auto-scroll to the bottom whenever messages change
  useEffect(() => {
    if (chatContainerRef.current) {
      chatContainerRef.current.scrollTop =
        chatContainerRef.current.scrollHeight;
    }
  }, [messages, isTyping]);

  return (
    <div className="app-container">
      {/* Header */}
      <div className="header">
        <h1>WebSocket Demo</h1>
        <button onClick={connectWebSocket}>Connect</button>
      </div>

      {/* Chat container */}
      <div className="chat-container" ref={chatContainerRef}>
        {messages.map((msg, index) => (
          <div
            key={index}
            className={`message-row ${msg.sender === "me" ? "me" : "server"}`}
          >
            <div className="message-bubble">{msg.components || msg.text}</div>
          </div>
        ))}
        {/* Typing Indicator */}
        {isTyping && (
          <div className="message-row server">
            <div className="typing-indicator">
              <span></span>
              <span></span>
              <span></span>
            </div>
          </div>
        )}
      </div>

      {/* Footer input */}
      <div className="chat-footer">
        <textarea
          value={inputMessage}
          onChange={(e) => setInputMessage(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type your message..."
          rows={1} // Start with single line
          className="message-input"
        />
        <button onClick={sendMessage}>Send</button>
      </div>
    </div>
  );
}

export default App;
