// dsh-launcher · 控制台前端(纯原生 JS,SSE 实时推送)
(() => {
  'use strict'

  const $ = (id) => document.getElementById(id)
  const logBody = $('log')
  const logEmpty = $('logEmpty')

  let config = { repoPath: '', port: 3080, host: '127.0.0.1' }
  let st = null // 状态快照
  let buffer = [] // 日志缓冲
  let lastId = 0
  let enabledSrc = new Set(['launcher', 'dsh web', 'dev:web', 'git', 'pnpm'])
  let paused = false

  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]
  ))

  const fmtTime = (ts) => {
    const d = new Date(ts)
    const p = (n) => String(n).padStart(2, '0')
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
  }

  const srcClass = (src) => ({
    'dsh web': 'src-dshweb', 'dev:web': 'src-devw',
    git: 'src-git', pnpm: 'src-pnpm', launcher: 'src-launcher',
  }[src] || 'src-launcher')

  // ── 渲染 ──────────────────────────────────────────────

  const PILL = {
    idle: ['idle', '空闲'], syncing: ['syncing', '同步中'],
    installing: ['installing', '安装依赖'], building: ['building', '构建中'],
    starting: ['starting', '启动中'], running: ['running', '运行中'],
    stopping: ['stopping', '停止中'], failed: ['failed', '失败'],
  }

  function renderState() {
    if (!st) return
    const pill = $('statusPill')
    const [cls, text] = PILL[st.state] || ['idle', st.state]
    pill.className = `status-pill ${cls}`
    $('statusText').textContent = text

    $('modeBadge').hidden = st.mode !== 'dev'
    $('modeBadge').textContent = '开发模式'

    const busy = st.busy || ['syncing', 'installing', 'building', 'starting', 'stopping'].includes(st.state)
    $('btnStart').disabled = busy
    $('btnDev').disabled = busy
    $('btnUpdate').disabled = busy
    $('btnRebuild').disabled = busy
    $('btnStop').disabled = !busy && st.state === 'idle'

    $('progress').classList.toggle('show', busy)
    $('phase').classList.toggle('show', !!st.phase)
    $('phase').textContent = st.phase || ''

    // 失败横幅
    const banner = $('errorBanner')
    if (st.error) {
      $('errorSummary').textContent = st.error.summary || '发生错误'
      $('errorDetail').textContent = st.error.detail || ''
      banner.hidden = false
    } else {
      banner.hidden = true
    }

    renderService()
    renderRepo()
  }

  function defaultUrl() {
    return `http://${config.host || '127.0.0.1'}:${String(config.port || 3080)}/`
  }

  function uptimeText() {
    if (st.state !== 'running' || !st.startedAt) return '未运行'
    const sec = Math.floor((Date.now() - st.startedAt) / 1000)
    if (sec < 60) return `已运行 ${sec}s`
    if (sec < 3600) return `已运行 ${Math.floor(sec / 60)}m`
    return `已运行 ${Math.floor(sec / 3600)}h ${Math.floor((sec % 3600) / 60)}m`
  }

  function renderService() {
    const url = st.url || defaultUrl()
    $('svcUrl').innerHTML = `<a href="${esc(url)}" target="_blank" rel="noopener">${esc(url)}</a>`
    if (st.state === 'running' && st.webPid) {
      const parts = [`PID ${st.webPid}`, `端口 ${new URL(url).port}`, uptimeText()]
      if (st.hmrActive) parts.push('HMR 活跃')
      $('svcMeta').textContent = parts.join(' · ')
    } else {
      $('svcMeta').textContent = st.state === 'running' ? '运行中(pid 未知)' : '未运行'
    }
  }

  const ago = (ts) => {
    if (!ts) return '尚未同步'
    const s = Math.floor((Date.now() - ts) / 1000)
    if (s < 60) return '刚刚'
    if (s < 3600) return `${Math.floor(s / 60)} 分钟前`
    return `${Math.floor(s / 3600)} 小时前`
  }

  function renderRepo() {
    const r = st.repo || {}
    const headEl = $('repoHead')
    const tag = $('repoTag')
    if (r.branch) {
      headEl.innerHTML = `<span class="mono">${esc(r.branch)}</span> @ <span class="mono">${esc(r.head || '—')}</span>`
      tag.hidden = false
      tag.className = `tag ${r.dirty ? 'dirty' : 'clean'}`
      tag.textContent = r.dirty ? '工作区有改动' : '工作区干净'
    } else {
      headEl.textContent = '—'
      tag.hidden = true
    }
    const behind = r.behind == null || r.behind < 0 ? '—' : String(r.behind)
    $('repoMeta').textContent = `落后 ${behind} 个提交 · 最近同步 ${ago(r.syncAt)}`
  }

  function renderLogs() {
    const enabled = enabledSrc
    const frag = document.createDocumentFragment()
    for (const e of buffer) {
      if (enabled.has(e.src)) frag.appendChild(lineEl(e))
    }
    logBody.replaceChildren(frag)
    logEmpty.classList.toggle('show', buffer.length === 0)
    scrollBottom()
  }

  function lineEl(e) {
    const div = document.createElement('div')
    div.className = 'log-line'
    const t = document.createElement('span')
    t.className = 't'
    t.textContent = fmtTime(e.ts)
    const s = document.createElement('span')
    s.className = `src ${srcClass(e.src)}`
    s.textContent = e.src
    const tx = document.createElement('span')
    if (e.level && e.level !== 'info') tx.className = `lvl-${e.level}`
    tx.textContent = e.text
    div.append(t, s, tx)
    return div
  }

  function appendLog(e) {
    buffer.push(e)
    if (buffer.length > 4000) buffer.splice(0, buffer.length - 4000)
    lastId = Math.max(lastId, e.id)
    if (enabledSrc.has(e.src)) {
      logEmpty.classList.toggle('show', false)
      logBody.appendChild(lineEl(e))
      while (logBody.children.length > 3000) logBody.firstChild.remove()
      if (!paused) scrollBottom()
    }
  }

  const nearBottom = () => logBody.scrollHeight - logBody.scrollTop - logBody.clientHeight < 60
  function scrollBottom() {
    logBody.scrollTop = logBody.scrollHeight
  }

  // ── 动作 ──────────────────────────────────────────────

  async function postJson(url, body) {
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body || {}),
    })
    return res.json()
  }

  function toast(msg, kind = '') {
    $('settingsMsg').className = `settings-msg ${kind}`
    $('settingsMsg').textContent = msg
    setTimeout(() => { $('settingsMsg').textContent = '' }, 4000)
  }

  async function doAction(action) {
    try {
      const r = await postJson('/api/action', { action })
      if (r.reason === 'busy') toast('上一个操作仍在进行,请稍候', 'err')
      else if (r.ok === false && r.reason === 'port-busy') toast('端口被占用 — 见上方诊断,请在设置中换端口', 'err')
      else if (r.ok === false) toast('操作未执行,详见日志与诊断横幅', 'err')
    } catch (err) {
      toast(`请求失败:${err.message}`, 'err')
    }
  }

  $('btnStart').addEventListener('click', () => doAction('start'))
  $('btnDev').addEventListener('click', () => doAction('dev'))
  $('btnUpdate').addEventListener('click', () => doAction('update'))
  $('btnStop').addEventListener('click', () => doAction('stop'))
  $('btnRebuild').addEventListener('click', () => doAction('rebuild'))
  $('btnClear').addEventListener('click', async () => {
    try {
      await postJson('/api/action', { action: 'clear' })
      const r = await fetch('/api/logs?since=0').then((x) => x.json())
      buffer = r.logs || []
      lastId = buffer.length ? buffer[buffer.length - 1].id : 0
      renderLogs()
    } catch { /* ignore */ }
  })

  $('btnOpen').addEventListener('click', () => {
    window.open(st && st.url ? st.url : defaultUrl(), '_blank')
  })

  $('errorClose').addEventListener('click', () => { $('errorBanner').hidden = true })

  // 来源筛选
  document.querySelectorAll('.log-head .chip[data-src]').forEach((chip) => {
    chip.addEventListener('click', () => {
      const src = chip.dataset.src
      if (enabledSrc.has(src)) enabledSrc.delete(src)
      else enabledSrc.add(src)
      chip.classList.toggle('on', enabledSrc.has(src))
      renderLogs()
    })
  })

  // 暂停自动滚动 / 置顶
  $('btnPause').addEventListener('click', () => {
    paused = !paused
    $('btnPause').textContent = paused ? '▶ 继续' : '⏸ 暂停'
    $('btnPause').classList.toggle('on', paused)
    if (!paused) scrollBottom()
  })
  $('btnTop').addEventListener('click', () => { logBody.scrollTop = 0 })

  // ── 设置 ──────────────────────────────────────────────

  function fillSettings() {
    $('setRepo').value = config.repoPath || ''
    $('setPort').value = String(config.port ?? 3080)
    $('setHost').value = config.host || '127.0.0.1'
    $('setDshHome').value = config.dshHome || ''
    $('setBuildArgs').value = config.buildArgs || ''
    $('setOpen').checked = !!config.openBrowser
    $('setAutostart').checked = !!config.autostart
  }

  $('btnSaveSettings').addEventListener('click', async () => {
    const patch = {
      repoPath: $('setRepo').value.trim(),
      port: Number($('setPort').value),
      host: $('setHost').value.trim() || '127.0.0.1',
      dshHome: $('setDshHome').value.trim(),
      buildArgs: $('setBuildArgs').value.trim(),
      openBrowser: $('setOpen').checked,
      autostart: $('setAutostart').checked,
    }
    try {
      const r = await postJson('/api/config', patch)
      if (r.ok) {
        config = r.config
        toast('设置已保存 ✓', 'ok')
        fillSettings()
        renderState()
      } else {
        toast(`保存失败:${r.reason}`, 'err')
      }
    } catch (err) {
      toast(`保存失败:${err.message}`, 'err')
    }
  })

  // ── 数据拉取与 SSE ────────────────────────────────────

  async function loadInitial() {
    try {
      const [cfgRes, stRes, logRes] = await Promise.all([
        fetch('/api/config').then((r) => r.json()),
        fetch('/api/state').then((r) => r.json()),
        fetch('/api/logs?since=0').then((r) => r.json()),
      ])
      if (cfgRes.ok) { config = cfgRes.config; fillSettings() }
      if (stRes.ok) { st = stRes.state; renderState() }
      if (logRes.ok) {
        buffer = logRes.logs || []
        lastId = buffer.length ? buffer[buffer.length - 1].id : 0
        renderLogs()
      }
      if (cfgRes.warnings && cfgRes.warnings.length) {
        buffer.unshift(...cfgRes.warnings.map((w, i) => ({
          id: -(i + 1), ts: Date.now(), src: 'launcher', level: 'warn', text: w,
        })))
        renderLogs()
      }
    } catch (err) {
      toast(`连接控制台失败:${err.message}`, 'err')
    }
  }

  function connectSSE() {
    const es = new EventSource('/api/events')
    let connectedOnce = false
    es.addEventListener('log', (ev) => {
      try { appendLog(JSON.parse(ev.data)) } catch { /* ignore */ }
    })
    es.addEventListener('state', (ev) => {
      try {
        st = JSON.parse(ev.data)
        renderState()
      } catch { /* ignore */ }
    })
    es.onopen = () => {
      if (!connectedOnce) { connectedOnce = true; return } // 首次打开由 loadInitial 负责
      // 断线重连后补拉状态与日志
      fetch('/api/state').then((r) => r.json()).then((r) => {
        if (r.ok) { st = r.state; renderState() }
      }).catch(() => {})
      fetch(`/api/logs?since=${lastId}`).then((r) => r.json()).then((r) => {
        if (r.ok) (r.logs || []).forEach(appendLog)
      }).catch(() => {})
    }
    es.onerror = () => { /* EventSource 自动重连 */ }
  }

  // 运行时长走秒
  setInterval(() => {
    if (st && st.state === 'running') renderService()
  }, 1000)

  loadInitial()
  connectSSE()
})()
