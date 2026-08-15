import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './index.css'

// 主题:跟随系统(默认);html.theme-light / .theme-dark 由设置页控制
const mq = window.matchMedia('(prefers-color-scheme: dark)')
function applyTheme() {
  const html = document.documentElement
  const forced = html.getAttribute('data-theme')
  if (forced === 'light') html.classList.remove('dark')
  else if (forced === 'dark') html.classList.add('dark')
  else html.classList.toggle('dark', mq.matches)
}
applyTheme()
mq.addEventListener('change', applyTheme)

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
