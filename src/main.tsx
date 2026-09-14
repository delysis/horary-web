import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ensureAiCoreLoaded } from './ai/aiCoreWasm.ts'

const root = createRoot(document.getElementById('root')!)
root.render(<p role="status">Loading Horary…</p>)
void ensureAiCoreLoaded().then(() => {
  root.render(<StrictMode><App /></StrictMode>)
}).catch(() => {
  root.render(<p role="alert">Horary could not load its calculation support. Check your connection and reload this page.</p>)
})
