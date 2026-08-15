import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { applyTheme } from '@/lib/theme'
import './index.css'

// 初始主题:跟随系统(偏好加载后由 App 覆盖)
applyTheme('system')

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
