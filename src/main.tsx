import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import HelpWindow from "./HelpWindow";
import LogWindow from "./LogWindow";
import PreferencesWindow from "./PreferencesWindow";
import ReceivedWindow from "./ReceivedWindow";

const hash = window.location.hash;
const isHelpWindow = hash === "#help";
const isLogWindow = hash === "#log";
const isPreferencesWindow = hash === "#preferences";
const isReceivedWindow = hash === "#received";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isHelpWindow ? <HelpWindow /> : isLogWindow ? <LogWindow /> : isPreferencesWindow ? <PreferencesWindow /> : isReceivedWindow ? <ReceivedWindow /> : <App />}
  </React.StrictMode>,
);
