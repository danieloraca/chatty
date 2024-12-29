import React, { useState } from "react";

function App() {
  const [socket, setSocket] = useState(null);
  const [inputMessage, setInputMessage] = useState("");
  const [receivedMessage, setReceivedMessage] = useState("");

  let messageText = "";
  const connectWebSocket = () => {
    // If you already have a socket open, you can close it first or just reuse it
    if (socket) {
      socket.close();
    }

    // Create the WebSocket
    const ws = new WebSocket("ws://127.0.0.1:3456/ws");

    // Socket opened
    ws.onopen = () => {
      console.log("WebSocket is open now");
      setReceivedMessage("Connected to the server!");
    };

    // Listen for messages
    ws.onmessage = (event) => {
      console.log("Received from server:", event.data);
      // Just replace the content of receivedMessage with the latest message
      messageText += event.data;
      setReceivedMessage(messageText);
    };

    // Socket closed
    ws.onclose = () => {
      console.log("WebSocket is closed now");
      setReceivedMessage("WebSocket connection closed");
    };

    // Socket error
    ws.onerror = (err) => {
      console.error("WebSocket error:", err);
      setReceivedMessage(`Error: ${err.message}`);
    };

    setSocket(ws);
  };

  const sendMessage = () => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(inputMessage);
      // Clear the input field after sending
      setInputMessage("");
    } else {
      setReceivedMessage("WebSocket is not open");
    }
  };

  return (
    <div style={{ margin: "2rem" }}>
      <h1>WebSocket Demo</h1>

      {/* Connect Button */}
      <div style={{ marginBottom: "1rem" }}>
        <button onClick={connectWebSocket}>Connect</button>
      </div>

      {/* Textbox to type a message */}
      <div style={{ marginBottom: "1rem" }}>
        <input
          type="text"
          value={inputMessage}
          onChange={(e) => setInputMessage(e.target.value)}
          placeholder="Type your message"
        />
        <button onClick={sendMessage}>Send Message</button>
      </div>

      {/* Display the latest server response */}
      <div
        style={{ border: "1px solid black", padding: "1rem", width: "300px" }}
      >
        <strong>Server Response:</strong>
        <p>{receivedMessage}</p>
      </div>
    </div>
  );
}

export default App;
