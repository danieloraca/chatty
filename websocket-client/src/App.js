import React, { useState, useRef, useEffect } from "react";
import "./App.css";

function App() {
  const [socket, setSocket] = useState(null);
  const [inputMessage, setInputMessage] = useState("");
  const [messages, setMessages] = useState([]);

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
      console.log("Received from server:", event.data);

      setMessages((prev) => {
        if (prev.length > 0 && prev[prev.length - 1].sender === "server") {
          // Clone the last message
          let lastMessage = { ...prev[prev.length - 1] };
          // Append the new data directly without adding an extra space
          lastMessage.text += event.data;
          // Return a new array with the updated last message
          return [...prev.slice(0, -1), lastMessage];
        } else {
          // If there's no last server message, create a new one
          return [...prev, { text: event.data, sender: "server" }];
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

  const sendMessage = () => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      // Send the message to the server
      socket.send(inputMessage);
      // Add it to our local messages
      setMessages((prev) => [...prev, { text: inputMessage, sender: "me" }]);
      // Clear the input field
      setInputMessage("");
    } else {
      // If not connected, handle accordingly
      setMessages((prev) => [
        ...prev,
        { text: "WebSocket is not open", sender: "server" },
      ]);
    }
  };

  // Optionally send message on Enter key
  const handleKeyDown = (e) => {
    if (e.key === "Enter") {
      sendMessage();
    }
  };

  // Auto-scroll to the bottom whenever messages change
  useEffect(() => {
    if (chatContainerRef.current) {
      chatContainerRef.current.scrollTop =
        chatContainerRef.current.scrollHeight;
    }
  }, [messages]);

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
            <div className="message-bubble">{msg.text}</div>
          </div>
        ))}
      </div>

      {/* Footer input */}
      <div className="chat-footer">
        <input
          type="text"
          value={inputMessage}
          onChange={(e) => setInputMessage(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type your message..."
        />
        <button onClick={sendMessage}>Send</button>
      </div>
    </div>
  );
}

export default App;
