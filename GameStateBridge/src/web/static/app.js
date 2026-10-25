const API_BASE = '';

let currentGame = null;
let refreshInterval = null;
let configStatus = { is_configured: false, config_path: null };
let configExpanded = false;
let cs2VisualHtml = null;

// 初始化
document.addEventListener('DOMContentLoaded', () => {
    cs2VisualHtml = document.getElementById('match-visual').innerHTML;
    initTabs();
    loadGames();
    initDashboardControls();
});

function initTabs() {
    document.querySelectorAll('.tab-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const tabId = btn.dataset.tab;
            switchTab(tabId);
        });
    });
}

function switchTab(tabId) {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
    document.querySelector(`.tab-btn[data-tab="${tabId}"]`).classList.add('active');
    document.getElementById(`${tabId}-tab`).classList.add('active');
}

async function loadGames() {
    try {
        const res = await fetch(`${API_BASE}/api/games`);
        const data = await res.json();
        renderGames(data.games);
    } catch (err) {
        console.error('Failed to load games:', err);
        document.getElementById('games-grid').innerHTML =
            '<p style="color:var(--danger)">加载失败，请检查服务是否运行</p>';
    }
}

function renderGames(games) {
    const grid = document.getElementById('games-grid');
    grid.innerHTML = games.map(g => `
        <div class="game-card" data-game="${g.game_type}">
            <div class="game-icon">${getGameIcon(g.game_type)}</div>
            <h3>${g.display_name}</h3>
            <div class="game-type">${g.game_type.toUpperCase()}</div>
            <span class="status-badge ${g.is_active ? 'active' : g.is_supported ? 'inactive' : 'unsupported'}">
                ${g.is_active ? '运行中' : g.is_supported ? '未启动' : '未实现'}
            </span>
        </div>
    `).join('');

    document.querySelectorAll('.game-card').forEach(card => {
        card.addEventListener('click', () => selectGame(card.dataset.game));
    });
}

function getGameIcon(gameType) {
    return { 'cs2': '🔫', 'lol': '⚔️' }[gameType] || '🎮';
}

async function selectGame(gameType) {
    currentGame = gameType;
    switchTab('dashboard');
    document.getElementById('dashboard-title').textContent =
        { 'cs2': 'CS2 仪表盘', 'lol': '英雄联盟 仪表盘' }[gameType] || '仪表盘';

    updateAutoConfigButton();
    await checkConfigStatus();
    await autoActivateIfNeeded();
    await refreshDashboard();
    startAutoRefresh();
}

/// 如果适配器未激活，自动激活
async function autoActivateIfNeeded() {
    try {
        const statusRes = await fetch(`${API_BASE}/api/games/${currentGame}/status`);
        const isActive = statusRes.ok;

        if (!isActive) {
            const body = currentGame === 'cs2'
                ? JSON.stringify({ listen_addr: '127.0.0.1:3001' })
                : JSON.stringify({});
            await fetch(`${API_BASE}/api/games/${currentGame}/activate`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body
            });
        }
    } catch (err) {
        console.error('Auto-activate failed:', err);
    }
}

function updateAutoConfigButton() {
    const btn = document.getElementById('btn-auto-config');
    btn.style.display = currentGame === 'cs2' ? 'inline-block' : 'none';
}

function initDashboardControls() {
    document.getElementById('btn-refresh').addEventListener('click', refreshDashboard);
    document.getElementById('btn-toggle').addEventListener('click', toggleAdapter);
    document.getElementById('btn-auto-config').addEventListener('click', autoConfigure);
    document.getElementById('btn-config-toggle').addEventListener('click', toggleConfig);
}

function toggleConfig() {
    configExpanded = !configExpanded;
    const content = document.getElementById('config-content');
    const btn = document.getElementById('btn-config-toggle');
    if (configExpanded) {
        content.classList.add('expanded');
        btn.textContent = '收起';
    } else {
        content.classList.remove('expanded');
        btn.textContent = '展开';
    }
}

async function checkConfigStatus() {
    if (currentGame !== 'cs2') return;
    try {
        const res = await fetch(`${API_BASE}/api/games/${currentGame}/config-status`);
        if (res.ok) configStatus = await res.json();
    } catch (err) {
        console.error('Failed to check config status:', err);
    }
}

async function autoConfigure() {
    if (currentGame !== 'cs2') return;
    const btn = document.getElementById('btn-auto-config');
    btn.disabled = true;
    btn.textContent = '配置中...';

    try {
        const res = await fetch(`${API_BASE}/api/games/${currentGame}/auto-config`, { method: 'POST' });
        const result = await res.json();
        alert(result.success ? '配置成功！\n' + result.message : '配置失败：\n' + result.message);
        if (result.success) {
            configStatus = { is_configured: true, config_path: result.config_path };
        }
        await refreshDashboard();
    } catch (err) {
        alert('配置请求失败: ' + err.message);
    } finally {
        btn.disabled = false;
        btn.textContent = '一键配置';
    }
}

async function toggleAdapter() {
    const btn = document.getElementById('btn-toggle');
    btn.disabled = true;

    try {
        const statusRes = await fetch(`${API_BASE}/api/games/${currentGame}/status`);
        const isActive = statusRes.ok;

        if (isActive) {
            await fetch(`${API_BASE}/api/games/${currentGame}/deactivate`, { method: 'POST' });
            btn.textContent = '激活';
            btn.className = 'btn btn-secondary';
        } else {
            const body = currentGame === 'cs2'
                ? JSON.stringify({ listen_addr: '127.0.0.1:3001' })
                : JSON.stringify({});
            await fetch(`${API_BASE}/api/games/${currentGame}/activate`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body
            });
            btn.textContent = '停用';
            btn.className = 'btn btn-danger';
        }

        await refreshDashboard();
        loadGames();
    } catch (err) {
        alert('操作失败: ' + err.message);
    } finally {
        btn.disabled = false;
    }
}

// 记录 LOL 上一次的对局阶段，用于“进入对局后自动触发评选”
let lastLolPhase = null;

async function refreshDashboard() {
    if (!currentGame) return;

    try {
        const statusRes = await fetch(`${API_BASE}/api/games/${currentGame}/status`);
        const isActive = statusRes.ok;
        const adapterStatus = isActive ? await statusRes.json() : null;

        updateConnectionBar(adapterStatus, isActive);
        updateToggleButton(isActive);

        if (isActive) {
            const stateRes = await fetch(`${API_BASE}/api/games/${currentGame}/state`);
            if (stateRes.ok) {
                const state = await stateRes.json();
                if (currentGame === 'lol') {
                    renderLolVisual(state);
                    if (state.match_phase === 'InProgress') {
                        updateLolLiveStatus();
                        // 刚进入对局时自动触发上等马评选（只触发一次）
                        if (lastLolPhase !== 'InProgress') {
                            lolCommand('evaluate');
                        }
                    } else {
                        clearLolLiveStatus();
                    }
                    lastLolPhase = state.match_phase;
                } else {
                    restoreCs2Visual();
                    renderMatchVisual(state);
                }
            } else {
                hideMatchVisual();
            }
        } else {
            hideMatchVisual();
        }

        updateConfigCard();
    } catch (err) {
        console.error('Refresh failed:', err);
    }
}

/* ===== 连接状态条 ===== */
function updateConnectionBar(status, isActive) {
    const bar = document.getElementById('connection-bar');
    const text = document.getElementById('conn-status-text');

    if (!isActive) {
        bar.className = 'connection-bar disconnected';
        text.textContent = '适配器未激活 — 点击"激活"按钮启动';
        return;
    }

    const lastUpdate = status.last_update
        ? new Date(status.last_update).toLocaleString('zh-CN')
        : '无';

    if (status.is_connected) {
        bar.className = 'connection-bar connected';
        text.textContent = `已连接 · 最后更新: ${lastUpdate}`;
    } else {
        bar.className = 'connection-bar disconnected';
        const err = status.error_message ? ` · ${status.error_message}` : '';
        text.textContent = `等待游戏连接中...${err} · 最后更新: ${lastUpdate}`;
    }
}

function updateToggleButton(isActive) {
    const btn = document.getElementById('btn-toggle');
    btn.textContent = isActive ? '停用' : '激活';
    btn.className = isActive ? 'btn btn-danger' : 'btn btn-secondary';
}

/* ===== 对局可视化 ===== */
function renderMatchVisual(state) {
    const visual = document.getElementById('match-visual');
    visual.style.display = 'block';

    // 地图与比分横幅
    const mapName = state.map_or_arena || '--';
    const gameMode = state.game_mode || '--';
    const ctScore = state.team_ct_score ?? '--';
    const tScore = state.team_t_score ?? '--';
    const phaseText = phaseDisplay(state.match_phase);

    document.getElementById('map-name').textContent = mapName;
    document.getElementById('game-mode').textContent = gameMode;
    document.getElementById('score-ct').textContent = ctScore;
    document.getElementById('score-t').textContent = tScore;
    document.getElementById('match-phase').textContent = phaseText;

    // 本地玩家面板
    if (state.local_player) {
        const p = state.local_player;
        document.getElementById('player-panel').style.display = 'block';
        document.getElementById('player-name').textContent = p.name || '未知玩家';

        const teamEl = document.getElementById('player-team');
        const teamClass = teamCss(p.team);
        teamEl.className = 'player-team ' + teamClass;
        teamEl.textContent = teamDisplay(p.team);

        const stats = p.stats || {};
        const health = stats.health ?? '--';
        const armor = stats.armor ?? '--';
        const money = stats.money ?? '--';

        document.getElementById('player-health').textContent = health;
        document.getElementById('player-armor').textContent = armor;
        document.getElementById('player-money').textContent = money;

        // KDA 展示逻辑：有累计数据则显示累计，否则显示本回合 + 提示
        const hasMatchStats = stats.kills !== null && stats.kills !== undefined;
        const hintEl = document.getElementById('kda-hint');

        if (hasMatchStats) {
            document.getElementById('label-kills').textContent = '击杀';
            document.getElementById('label-deaths').textContent = '死亡';
            document.getElementById('label-assists').textContent = '助攻';
            document.getElementById('label-mvps').textContent = 'MVP';
            document.getElementById('player-kills').textContent = stats.kills ?? '--';
            document.getElementById('player-deaths').textContent = stats.deaths ?? '--';
            document.getElementById('player-assists').textContent = stats.assists ?? '--';
            document.getElementById('player-mvps').textContent = stats.mvps ?? stats.extra?.mvps ?? '--';
            hintEl.style.display = 'none';
        } else {
            const roundKills = stats.round_kills ?? '--';
            const roundKillhs = stats.round_killhs ?? '--';
            document.getElementById('label-kills').textContent = '本回合击杀';
            document.getElementById('label-deaths').textContent = '本回合爆头';
            document.getElementById('label-assists').textContent = '累计击杀';
            document.getElementById('label-mvps').textContent = '累计死亡';
            document.getElementById('player-kills').textContent = roundKills;
            document.getElementById('player-deaths').textContent = roundKillhs;
            document.getElementById('player-assists').textContent = '--';
            document.getElementById('player-mvps').textContent = '--';
            hintEl.textContent = '休闲模式不推送累计战绩，切换竞技模式可查看完整 K/D/A';
            hintEl.style.display = 'block';
        }

        // 血条/护甲条
        const healthBar = document.getElementById('health-bar');
        const armorBar = document.getElementById('armor-bar');
        healthBar.style.width = (typeof health === 'number' ? Math.min(health, 100) : 0) + '%';
        armorBar.style.width = (typeof armor === 'number' ? Math.min(armor, 100) : 0) + '%';

        // 武器信息
        const weapon = p.hero_or_agent;
        document.getElementById('weapon-name').textContent = weapon || '无';

        // 尝试从 raw_payload 获取弹药信息
        let ammoText = '';
        if (state.raw_payload && state.raw_payload.player && state.raw_payload.player.weapons) {
            const weapons = state.raw_payload.player.weapons;
            for (const [key, w] of Object.entries(weapons)) {
                if (w.state === 'active' && w.name) {
                    const clip = w.ammo_clip ?? '-';
                    const reserve = w.ammo_reserve ?? '-';
                    ammoText = `${clip} / ${reserve}`;
                    break;
                }
            }
        }
        document.getElementById('weapon-ammo').textContent = ammoText;
    } else {
        document.getElementById('player-panel').style.display = 'none';
    }

    // 回合信息
    const roundNum = state.round ?? '--';
    const roundPhase = state.round_phase || '--';
    document.getElementById('round-num').textContent = roundNum;
    document.getElementById('round-phase').textContent = roundPhaseDisplay(roundPhase);

    // 所有玩家列表
    const table = document.getElementById('players-table');
    if (state.players && state.players.length > 0) {
        document.getElementById('allplayers-section').style.display = 'block';
        table.innerHTML = state.players.map(p => {
            const isLocal = state.local_player && (
                (state.local_player.steam_id && p.steam_id && state.local_player.steam_id === p.steam_id) ||
                (!state.local_player.steam_id && state.local_player.name === p.name)
            );
            const tClass = teamCss(p.team);
            const stats = p.stats || {};
            return `
                <div class="player-row ${tClass} ${isLocal ? 'local' : ''}">
                    <span class="player-row-name">${p.name}${isLocal ? ' (你)' : ''}</span>
                    <span class="player-row-team ${tClass}">${teamDisplay(p.team)}</span>
                    <div class="player-row-stats">
                        <div class="player-row-stat">
                            <span class="label">K</span><span class="value">${stats.kills ?? '-'}</span>
                        </div>
                        <div class="player-row-stat">
                            <span class="label">A</span><span class="value">${stats.assists ?? '-'}</span>
                        </div>
                        <div class="player-row-stat">
                            <span class="label">D</span><span class="value">${stats.deaths ?? '-'}</span>
                        </div>
                        <div class="player-row-stat">
                            <span class="label">HP</span><span class="value">${stats.health ?? '-'}</span>
                        </div>
                    </div>
                </div>
            `;
        }).join('');
    } else {
        document.getElementById('allplayers-section').style.display = 'none';
    }
}

function hideMatchVisual() {
    document.getElementById('match-visual').style.display = 'none';
}

/* ===== LOL 专属仪表盘 ===== */
function restoreCs2Visual() {
    const visual = document.getElementById('match-visual');
    if (visual.dataset.mode === 'lol') {
        visual.dataset.mode = '';
        visual.innerHTML = cs2VisualHtml;
    }
}

function ensureLolSkeleton() {
    const visual = document.getElementById('match-visual');
    if (visual.dataset.mode === 'lol') return;
    visual.dataset.mode = 'lol';
    visual.innerHTML = `
        <div class="lol-panel">
            <div class="lol-topbar">
                <div class="lol-topbar-left">
                    <span id="lol-phase-badge" class="status-badge inactive">菜单</span>
                    <span id="lol-conn-info" class="lol-conn-info"></span>
                    <span id="lol-live" class="lol-live"></span>
                </div>
                <span class="lol-note">LCU 本地接口 · 端口自动发现</span>
            </div>

            <div class="lol-card">
                <h3 class="lol-card-title">快捷操作</h3>
                <div class="lol-toolbar">
                    <button class="btn btn-primary" onclick="lolCommand('start_match')">一键匹配</button>
                    <button class="btn btn-secondary" onclick="lolCommand('stop_match')">取消匹配</button>
                    <button class="btn btn-primary" onclick="lolCommand('accept_match')">接受对局</button>
                    <button class="btn btn-danger" onclick="lolCommand('evaluate')">🐎 上等马评选</button>
                    <button class="btn btn-secondary" onclick="lolPromptChat()">发送聊天</button>
                </div>
            </div>

            <div id="lol-result"></div>

            <div class="lol-card">
                <h3 class="lol-card-title">对战战绩</h3>
                <div id="lol-players" class="lol-players-box"></div>
            </div>

            <div class="lol-card">
                <h3 class="lol-card-title">LOL 配置</h3>
                <div class="lol-form">
                    <div class="lol-field">
                        <label for="lol-mode-name">模式名（模糊匹配）</label>
                        <input id="lol-mode-name" type="text" placeholder="如 海克斯大乱斗">
                    </div>
                    <div class="lol-field">
                        <label for="lol-queue-select">队列（优先于模式名）</label>
                        <div class="lol-field-inline">
                            <select id="lol-queue-select">
                                <option value="">— 按模式名自动匹配 —</option>
                            </select>
                            <button class="btn btn-secondary btn-sm" onclick="lolCommand('list_queues')">刷新队列</button>
                        </div>
                    </div>
                    <div class="lol-field">
                        <label for="lol-douyin-url">抖音 URL</label>
                        <input id="lol-douyin-url" type="text" placeholder="https://www.douyin.com">
                    </div>
                    <div class="lol-toggles">
                        <label class="lol-check"><input type="checkbox" id="lol-death-switch"> 死亡跳抖音</label>
                        <label class="lol-check"><input type="checkbox" id="lol-chat-switch"> 评选发聊天</label>
                        <label class="lol-check"><input type="checkbox" id="lol-auto-accept"> 自动接受对局</label>
                    </div>
                    <div class="lol-form-actions">
                        <button class="btn btn-primary" onclick="saveLolConfig()">保存配置</button>
                    </div>
                </div>
            </div>
        </div>`;
    // 首次渲染时拉一次配置与可玩队列列表（用于下拉选择）
    lolCommand('get_config');
    lolCommand('list_queues');
}

function renderLolVisual(state) {
    ensureLolSkeleton();
    const visual = document.getElementById('match-visual');
    visual.style.display = 'block';

    const badge = document.getElementById('lol-phase-badge');
    if (badge) {
        const phase = phaseDisplay(state.match_phase);
        badge.textContent = phase;
        const live = ['live', 'loading', 'intermission'].includes(state.match_phase);
        badge.className = 'status-badge ' + (live ? 'active' : 'inactive');
    }

    renderLolPlayers(state.players || []);
}

async function updateLolLiveStatus() {
    try {
        const res = await fetch(`${API_BASE}/api/games/lol/command`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ cmd: 'live_status' }),
        });
        const d = await res.json();
        const el = document.getElementById('lol-live');
        if (!el) return;
        if (!d.in_game) {
            el.className = 'lol-live';
            el.innerHTML = '';
            return;
        }
        if (d.dead) {
            const t = Math.max(0, Math.ceil(d.respawn_timer || 0));
            el.className = 'lol-live dead';
            el.innerHTML = `💀 阵亡 · 复活 ${t}s`;
        } else {
            el.className = 'lol-live alive';
            el.innerHTML = `🟢 存活 · ${d.champion || ''} ${d.kills}/${d.deaths}/${d.assists} · Lv.${d.level}`;
        }
    } catch (e) {
        /* 非对局中或接口不可用，忽略 */
    }
}

function clearLolLiveStatus() {
    const el = document.getElementById('lol-live');
    if (el) {
        el.className = 'lol-live';
        el.innerHTML = '';
    }
}

function renderLolPlayers(players) {
    const el = document.getElementById('lol-players');
    if (!el) return;
    if (!players.length) {
        el.innerHTML = '<div class="lol-empty-placeholder">无数据</div>';
        return;
    }
    const blue = players.filter(p => p.team === 'blue');
    const red = players.filter(p => p.team === 'red');
    const col = (list, title, cls) => `
        <div class="lol-team ${cls}">
            <div class="lol-team-head">${title}<span class="lol-team-count">${list.length}</span></div>
            ${list.map(p => {
                const extra = p.stats || {};
                const tier = extra.tier || '未定级';
                const perf = extra.perf || '';
                const recent = extra.recent || '';
                return `
                <div class="lol-player-row">
                    <div class="lol-player-top">
                        <span class="lol-player-name">${p.name || '(未知)'}</span>
                        <span class="lol-player-score">${extra.score != null ? '★' + extra.score : ''}</span>
                    </div>
                    <div class="lol-player-meta">
                        <span class="lol-player-tier">${tier}</span>
                        ${perf ? `<span class="lol-player-perf">${perf}</span>` : ''}
                        ${recent ? `<span class="lol-player-recent" title="近5场 击杀/死亡/助攻/胜负">${recent}</span>` : ''}
                    </div>
                </div>`;
            }).join('') || '<div class="lol-empty">暂无</div>'}
        </div>`;
    el.innerHTML = `<div class="lol-teams">${col(blue, '我方', 'ct')}${col(red, '对方', 'tr')}</div>`;
}

function showLolResult(html, isError = false) {
    const el = document.getElementById('lol-result');
    if (!el) return;
    el.innerHTML = `<div class="lol-result ${isError ? 'error' : 'success'}">${html}</div>`;
}

/// 复制指定文本域内容到剪贴板（自行粘贴到游戏聊天）
function copyLolText(id) {
    const ta = document.getElementById(id);
    if (!ta) return;
    ta.focus();
    ta.select();
    ta.setSelectionRange(0, ta.value.length);
    try {
        navigator.clipboard.writeText(ta.value);
    } catch (e) {
        document.execCommand('copy');
    }
}

async function lolCommand(cmd, args = {}) {
    try {
        const res = await fetch(`${API_BASE}/api/games/lol/command`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ cmd, args })
        });
        const data = await res.json();
        if (!res.ok) {
            showLolResult(`<b>失败</b><br><code>${(data.error || JSON.stringify(data)).replace(/</g, '&lt;')}</code>`, true);
            return null;
        }
        if (cmd === 'get_config') {
            fillLolConfig(data);
        } else if (cmd === 'evaluate') {
            const note = data.sent ? '✅ 已自动发送到游戏内聊天' : '⚠️ 未自动发送（可在下方复制内容自己发）';
            const rows = (data.players || []).map((p, i) => {
                const cls = p.horse === '上等马' ? 'top' : (p.horse === '下等马' ? 'bottom' : 'mid');
                const perf = p.perf_label ? ` · ${p.perf_label}` : '';
                const tier = p.tier_label || '未定级';
                return `<div class="lol-horse ${cls}">
                    <span class="lol-horse-rank">#${i + 1}</span>
                    <span class="lol-horse-name">${p.name}</span>
                    <span class="lol-horse-tier">${tier}${perf}</span>
                    <span class="lol-horse-tag">${p.horse}</span>
                </div>`;
            }).join('');
            const chatText = (data.message || '')
                .replace(/&/g, '&amp;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;');
            showLolResult(`<b>上等马评选（按战力自动分档）</b>${rows || '<div class="lol-empty">暂无数据</div>'}
                <div class="lol-chatbox">
                    <div class="lol-chatbox-head">📋 聊天内容（复制后自己发到游戏）</div>
                    <textarea id="lol-chat-text" class="lol-chat-text" readonly>${chatText}</textarea>
                    <button class="btn btn-secondary btn-sm" onclick="copyLolText('lol-chat-text')">复制</button>
                </div>
                <div class="lol-note" style="margin-top:8px;">${note}</div>`);
        } else if (cmd === 'start_match') {
            showLolResult(`✅ 已开始匹配（queueId=${data.queue_id}）`);
        } else if (cmd === 'list_queues') {
            const playable = (Array.isArray(data) ? data : [])
                .filter(q => (q.queueAvailability || 'Available') === 'Available')
                .slice(0, 80);
            populateLolQueueSelect(playable);
        } else {
            showLolResult(`✅ ${cmd} 成功`);
        }
        return data;
    } catch (err) {
        showLolResult(`<b>请求失败</b><br><code>${err.message}</code>`, true);
        return null;
    }
}

function populateLolQueueSelect(list) {
    const sel = document.getElementById('lol-queue-select');
    if (!sel) return;
    const cur = sel.value;
    sel.innerHTML = '<option value="">— 按模式名自动匹配 —</option>';
    for (const q of list) {
        const opt = document.createElement('option');
        opt.value = String(q.id);
        opt.textContent = `${q.id}:${q.name || q.shortName}`;
        sel.appendChild(opt);
    }
    // 保留当前选择（若仍在新列表中）
    if (cur) sel.value = cur;
}

function fillLolConfig(cfg) {
    if (!cfg || !cfg.mode_name) return;
    document.getElementById('lol-mode-name').value = cfg.mode_name || '';
    document.getElementById('lol-douyin-url').value = cfg.douyin_url || '';
    document.getElementById('lol-death-switch').checked = !!cfg.death_switch_enabled;
    document.getElementById('lol-chat-switch').checked = !!cfg.chat_enabled;
    document.getElementById('lol-auto-accept').checked = !!cfg.auto_accept_enabled;
    const sel = document.getElementById('lol-queue-select');
    if (sel && cfg.queue_id != null) sel.value = String(cfg.queue_id);
}

async function saveLolConfig() {
    const qsel = document.getElementById('lol-queue-select').value;
    const args = {
        mode_name: document.getElementById('lol-mode-name').value.trim(),
        douyin_url: document.getElementById('lol-douyin-url').value.trim(),
        death_switch_enabled: document.getElementById('lol-death-switch').checked,
        chat_enabled: document.getElementById('lol-chat-switch').checked,
        auto_accept_enabled: document.getElementById('lol-auto-accept').checked,
        queue_id: qsel === '' ? null : parseInt(qsel, 10),
    };
    const data = await lolCommand('set_config', args);
    if (data) showLolResult('✅ 配置已保存（死亡监视器实时生效）');
}

function lolPromptChat() {
    const msg = prompt('发送到游戏内聊天的内容：');
    if (msg) lolCommand('send_chat', { message: msg });
}

function renderHorses(players) {
    const el = document.getElementById('lol-players');
    if (!el || !players.length) return;
    const tier = i => i < 3 ? 'top' : (i >= players.length - 3 ? 'bottom' : 'mid');
    const rows = players.map((p, i) => `
        <div class="lol-horse ${tier(i)}">
            <span class="lol-horse-rank">#${i + 1}</span>
            <span class="lol-horse-name">${p.name} <span class="lol-horse-lv">Lv.${p.level}</span></span>
            <span class="lol-horse-tier" title="${p.perf_label || ''}">${p.tier_label || '未定级'}<br><span class="lol-horse-perf">${p.perf_label || ''}</span></span>
        </div>`).join('');
    el.innerHTML = `<div class="lol-card"><h3 class="lol-card-title">上等马评选结果</h3>${rows}</div>`;
}

/* ===== 配置卡片 ===== */
function updateConfigCard() {
    const card = document.getElementById('config-card');
    const content = document.getElementById('config-content');
    const header = document.getElementById('config-header');
    const toggleBtn = document.getElementById('btn-config-toggle');

    if (currentGame !== 'cs2') {
        card.style.display = 'none';
        return;
    }

    card.style.display = 'block';

    // 已配置时默认折叠
    if (configStatus.is_configured && !configExpanded) {
        content.classList.remove('expanded');
        toggleBtn.textContent = '展开';
    } else if (configStatus.is_configured && configExpanded) {
        content.classList.add('expanded');
        toggleBtn.textContent = '收起';
    } else if (!configStatus.is_configured) {
        // 未配置时自动展开
        configExpanded = true;
        content.classList.add('expanded');
        toggleBtn.textContent = '收起';
    }

    const badge = configStatus.is_configured
        ? '<span class="status-badge active" style="margin-left:8px;">已配置</span>'
        : '<span class="status-badge inactive" style="margin-left:8px;">未配置</span>';

    const pathLine = configStatus.config_path
        ? `<p style="color:var(--success);margin-top:6px;">路径: <code>${configStatus.config_path}</code></p>`
        : '';

    content.innerHTML = `
        <div class="config-step">
            <h4>配置状态 ${badge}</h4>
            ${pathLine}
            ${!configStatus.is_configured
                ? '<p style="color:var(--warning)">未检测到配置文件，请点击上方"一键配置"按钮自动创建。</p>'
                : '<p style="color:var(--success)">配置文件已就绪，请重启 CS2 或执行 <code>exec gamestate_integration_bridge</code> 加载配置。</p>'}
        </div>
        <div class="config-step">
            <h4>手动配置步骤</h4>
            <p>如果自动配置失败，可手动创建配置文件：</p>
            <p><code>Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg/gamestate_integration_bridge.cfg</code></p>
        </div>
        <div class="config-step">
            <h4>配置文件内容</h4>
            <div class="config-code">"GameStateIntegration"
{
    "uri"           "http://127.0.0.1:3001/gsi/cs2"
    "timeout"       "5.0"
    "buffer"        "0.1"
    "throttle"      "0.1"
    "heartbeat"     "30.0"
    "data"
    {
        "provider"              "1"
        "map"                   "1"
        "round"                 "1"
        "player_id"             "1"
        "player_state"          "1"
        "player_weapons"        "1"
        "player_match_stats"    "1"
        "allplayers_id"         "1"
        "allplayers_state"      "1"
        "allplayers_match_stats" "1"
        "allplayers_weapons"    "1"
        "allplayers_position"   "1"
        "phase_countdowns"      "1"
        "allgrenades"           "1"
        "bomb"                  "1"
    }
}</div>
        </div>
        <div class="config-step">
            <h4>加载配置</h4>
            <p>在 CS2 控制台执行：</p>
            <p><code>exec gamestate_integration_bridge</code></p>
        </div>
    `;
}

/* ===== 辅助函数 ===== */
function phaseDisplay(phase) {
    const map = {
        'menu': '菜单',
        'loading': '加载中',
        'warmup': '热身',
        'live': '进行中',
        'intermission': '中场',
        'gameover': '结束',
        'unknown': '未知'
    };
    return map[phase] || phase;
}

function roundPhaseDisplay(phase) {
    const map = {
        'freezetime': '冻结时间',
        'live': '回合中',
        'over': '回合结束',
        'bomb': '炸弹已安放',
        'defused': '炸弹已拆除'
    };
    return map[phase] || phase;
}

function teamDisplay(team) {
    const map = { 'ct': 'CT', 'terrorist': 'T', 'blue': '蓝', 'red': '红', 'spectator': '观战', 'unknown': '?' };
    return map[team] || team;
}

function teamCss(team) {
    const map = { 'ct': 'ct', 'terrorist': 'terrorist', 'blue': 'ct', 'red': 'terrorist' };
    return map[team] || 'unknown';
}

function startAutoRefresh() {
    stopAutoRefresh();
    refreshInterval = setInterval(refreshDashboard, 2000);
}

function stopAutoRefresh() {
    if (refreshInterval) {
        clearInterval(refreshInterval);
        refreshInterval = null;
    }
}

/* ===== Overlay 配置 ===== */
document.addEventListener('DOMContentLoaded', () => {
    initOverlayControls();
    loadOverlayConfig();
});

function initOverlayControls() {
    document.getElementById('btn-save-overlay')?.addEventListener('click', saveOverlayConfig);
    document.getElementById('btn-test-show')?.addEventListener('click', () => testOverlay(true));
    document.getElementById('btn-test-hide')?.addEventListener('click', () => testOverlay(false));

}

async function loadOverlayConfig() {
    try {
        const res = await fetch(`${API_BASE}/api/overlay/config`);
        if (res.ok) {
            const config = await res.json();
            document.getElementById('overlay-enabled').checked = config.enabled;
            document.getElementById('overlay-url').value = config.url;
            document.getElementById('overlay-player-steam-id').value = config.player_steam_id || '';
            if (config.player_name) {
                document.getElementById('overlay-player-name').value = config.player_name;
            }
        }
    } catch (err) {
        console.error('Failed to load overlay config:', err);
    }
}

async function saveOverlayConfig() {
    const btn = document.getElementById('btn-save-overlay');
    btn.disabled = true;
    btn.textContent = '保存中...';

    try {
        const config = {
            enabled: document.getElementById('overlay-enabled').checked,
            url: document.getElementById('overlay-url').value,
            player_steam_id: document.getElementById('overlay-player-steam-id').value.trim(),
            player_name: document.getElementById('overlay-player-name').value,
        };

        const res = await fetch(`${API_BASE}/api/overlay/config`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(config)
        });

        if (res.ok) {
            alert('配置已保存，窗口将重新加载');
        } else {
            alert('保存失败');
        }
    } catch (err) {
        alert('保存失败: ' + err.message);
    } finally {
        btn.disabled = false;
        btn.textContent = '保存配置';
    }
}

async function testOverlay(show) {
    try {
        const endpoint = show ? '/api/overlay/show' : '/api/overlay/hide';
        const res = await fetch(`${API_BASE}${endpoint}`, { method: 'POST' });
        if (res.ok) {
            console.log(show ? 'Overlay test: show' : 'Overlay test: hide');
        }
    } catch (err) {
        alert('测试失败: ' + err.message);
    }
}
