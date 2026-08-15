// dsh-launcher · 控制台前端(纯原生 JS,SSE 实时推送)
(() => {
  'use strict'

  const $ = (id) => document.getElementById(id)
  const logBody = $('log')
  const logEmpty = $('logEmpty')

  let config = { repoPath: '', port: 3080, host: '127.0.0.1' }
  let st = null // 状态快照
  let toolsNode = null // /api/config 里的 node 信息
  let buffer = [] // 日志缓冲
  let lastId = 0
  let enabledSrc = new Set(['launcher', 'dsh web', 'dev:web', 'git', 'pnpm'])
  let paused = false
  const NODE_RANGE_TEXT = '^22.19 || >=24'

  // 删除条件:legacy 控制台随桌面版稳定后移除(token 由 daemon 注入页面)
  const TOKEN = window.__DSH_LAUNCHER_TOKEN__ || ''
  const authHeaders = () => (TOKEN ? { Authorization: `Bearer ${TOKEN}` } : {})
  if (TOKEN) {
    const rawFetch = window.fetch.bind(window)
    window.fetch = (url, opts = {}) => rawFetch(url, {
      ...opts,
      headers: { ...(opts.headers || {}), ...authHeaders() },
    })
  }

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
    const installBtn = $('btnInstallNode')
    if (installBtn) installBtn.disabled = busy || st.state === 'starting'

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

    renderUpdate()
    renderService()
    renderRepo()
  }

  // ── 内置更新 ──────────────────────────────────────────
  function renderUpdate() {
    const up = st.update || {}
    const pill = $('updatePill')
    if (up.available && !up.installing) {
      pill.hidden = false
      pill.classList.remove('installing')
      $('updatePillText').textContent = `新版本 ${up.version} · 更新`
    } else if (up.installing) {
      pill.hidden = true
      const bar = $('updateProgress')
      bar.hidden = false
      const p = up.progress ?? 0
      $('updateProgressBar').style.width = `${p}%`
      $('updateMsg').textContent = up.progress >= 99 ? '解压并切换中…' : `下载更新 ${p}%`
    } else {
      pill.hidden = true
      $('updateProgress').hidden = true
    }
    if (up.mode) {
      $('upMode').textContent = { git: 'git 检出', app: 'macOS App', portable: '便携包', dev: '开发运行' }[up.mode] || up.mode
      $('upVersion').textContent = st.version || '—'
    }
    if (!up.installing && up.message && !up.available) {
      $('updateMsg').textContent = up.message
      $('updateMsg').className = 'update-msg' + (up.error ? ' err' : ' ok')
    }
  }

  function updatePillClick() {
    const up = st.update || {}
    if (!up.available || up.installing) return
    if (!confirm(`发现新版本 ${up.version}(当前 ${st.version})\n\n点击确定即下载并安装,完成后启动器自动重启(dsh web 服务不受影响)。`)) return
    postJson('/api/update', { action: 'apply' }).then((r) => {
      if (r.ok === false && r.error) toast(`更新失败:${r.error}`, 'err')
    }).catch((err) => toast(`更新请求失败:${err.message}`, 'err'))
  }

  $('updatePill').addEventListener('click', updatePillClick)

  $('btnCheckUpdate').addEventListener('click', async () => {
    const msg = $('updateMsg')
    msg.className = 'update-msg'
    msg.textContent = '检查中…'
    try {
      const r = await postJson('/api/update', { action: 'check' })
      const up = r.update || {}
      if (up.available) {
        msg.className = 'update-msg ok'
        msg.textContent = `发现新版本 ${up.version} — 点击顶部横幅一键更新`
      } else {
        msg.className = 'update-msg' + (up.error ? ' err' : ' ok')
        msg.textContent = up.message || (up.error ? `检查失败:${up.error}` : '已是最新版本')
      }
    } catch (err) {
      msg.className = 'update-msg err'
      msg.textContent = `检查失败:${err.message}`
    }
  })

  function defaultUrl() {
    return `http://${config.host || '127.0.0.1'}:${String(config.port || 3080)}/`
  }

  // ── 仓库构建状态提示 ──────────────────────────────────
  function updateRepoHint(distBuilt, usable) {
    const hint = $('repoHint')
    if (!usable || !usable.ok) {
      hint.className = 'repo-hint err'
      hint.textContent = usable && usable.reason ? `仓库不可用:${usable.reason}` : '仓库路径未配置'
      return
    }
    if (distBuilt === true) {
      hint.className = 'repo-hint ok'
      hint.textContent = '✓ 前端已构建,可直接点「启动」,无需先构建'
    } else {
      hint.className = 'repo-hint warn'
      hint.textContent = '⚠ 前端 dist 未构建 — 首次请先点「更新并构建」,之后即可直接「启动」'
    }
  }

  // ── 退出启动器 ────────────────────────────────────────
  $('btnQuit').addEventListener('click', async () => {
    if (!confirm('退出启动器?\n\n将停止 dsh web / dev:web 等全部托管进程,并关闭启动器服务本身。')) return
    try {
      await postJson('/api/action', { action: 'quit' })
    } catch { /* 服务可能已退出,忽略 */ }
    $('quitOverlay').hidden = false
  })
  $('btnReopen').addEventListener('click', () => {
    window.location.reload()
    // 若服务已重启,reload 会重新连上;否则提示
  })

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
    $('setAutoUpdate').checked = config.autoUpdateCheck !== false
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
      autoUpdateCheck: $('setAutoUpdate').checked,
    }
    try {
      const r = await postJson('/api/config', patch)
      if (r.ok) {
        config = r.config
        toast('设置已保存 ✓', 'ok')
        fillSettings()
        renderState()
        // 仓库路径可能变了,重新拉构建状态
        fetch('/api/config').then((x) => x.json()).then((x) => {
          if (x.ok) updateRepoHint(x.distBuilt, x.usable)
        }).catch(() => {})
      } else {
        toast(`保存失败:${r.reason}`, 'err')
      }
    } catch (err) {
      toast(`保存失败:${err.message}`, 'err')
    }
  })

  // ── Node 运行时状态 ────────────────────────────────────
  function renderNode() {
    const row = $('nodeRow')
    if (!row || !toolsNode) { if (row) row.hidden = true; return }
    row.hidden = false
    const dot = $('nodeDot')
    const text = $('nodeText')
    const btn = $('btnInstallNode')
    if (toolsNode.inRange) {
      row.className = 'node-row ok'
      dot.className = 'node-dot'
      text.textContent = `Node ${toolsNode.current} 符合 dsh 要求(${toolsNode.current})`
      btn.hidden = true
    } else if (toolsNode.used) {
      row.className = 'node-row ok'
      dot.className = 'node-dot'
      text.textContent = `当前 Node ${toolsNode.current} 不在 dsh 范围,已自动选用 Node ${toolsNode.usedVersion}`
      const path = document.createElement('span')
      path.className = 'node-path'
      path.textContent = toolsNode.used
      text.appendChild(path)
      btn.hidden = true
    } else {
      row.className = 'node-row err'
      dot.className = 'node-dot'
      text.textContent = `Node ${toolsNode.current} 不在 dsh 范围(${NODE_RANGE_TEXT})— 开发模式 / 构建不可用`
      btn.hidden = false
    }
  }

  $('btnInstallNode').addEventListener('click', async () => {
    if (!confirm('将下载官方 Node 24 LTS(约 50MB)并安装到启动器托管目录,安装后自动选用。继续?')) return
    const btn = $('btnInstallNode')
    btn.disabled = true
    try {
      const r = await postJson('/api/action', { action: 'install-node' })
      if (r.ok === false && r.reason === 'busy') {
        toast('上一个操作仍在进行,请稍候', 'err')
      } else if (r.ok === false) {
        toast('安装失败,详见日志与诊断横幅', 'err')
      }
      // 等安装完成后(状态离开 starting/busy)拉一次最新配置,刷新 Node 状态
      const poll = async () => {
        const s = await fetch('/api/state').then((x) => x.json()).catch(() => null)
        if (s && s.ok && s.state.busy) { setTimeout(poll, 1500); return }
        const c = await fetch('/api/config').then((x) => x.json()).catch(() => null)
        if (c && c.ok) { toolsNode = c.tools && c.tools.node; renderNode() }
        btn.disabled = false
      }
      setTimeout(poll, 1200)
    } catch (err) {
      toast(`安装请求失败:${err.message}`, 'err')
      btn.disabled = false
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
      if (cfgRes.ok) {
        config = cfgRes.config
        toolsNode = cfgRes.tools && cfgRes.tools.node
        fillSettings()
        updateRepoHint(cfgRes.distBuilt, cfgRes.usable)
        renderNode()
      }
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
    const es = new EventSource(`/api/events?token=${TOKEN}`)
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
