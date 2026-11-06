<script setup lang="ts">
// 云同步面板（docs/sync-protocol.md §8.4）：开关 / 同步口令 / 配对二维码 / 设备列表 / 立即同步。
//
// 挂在「数据与关于」大类的 DataPanel 里（id=sv-sec-sync，设置搜索索引构建期自动收录）。
// 身份与密钥分开：二维码只含一次性配对凭据；口令只在本机 scrypt 派生，永不传输——
// 所以开启 = 设口令（sync_enable），配对 = 出码等手机扫码（begin/poll/cancel），两件事互不阻塞。
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue';
import QRCode from 'qrcode';
import {
  Lock,
  Monitor,
  RefreshCw,
  Smartphone,
  QrCode as QrCodeIcon,
  X,
} from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { SyncDeviceInfo, SyncStatus } from '../../api/tauri';
import { useStore } from '../../stores/workbench';

const store = useStore();
const showToast = inject<(msg: string) => void>('showToast', () => {});

const status = ref<SyncStatus | null>(null);
const loading = ref(false);

/** 上次成功同步时间（显示用；后端不落库，记在 localStorage） */
const LAST_SYNC_KEY = 'xhub-sync-last-at';
const lastSyncAt = ref(0);

async function loadStatus() {
  if (!isTauri()) return;
  loading.value = true;
  try {
    status.value = await tauriApi.syncStatus();
    lastSyncAt.value = Number(localStorage.getItem(LAST_SYNC_KEY) || 0);
    if (status.value.paired) void loadDevices();
  } catch (e) {
    showToast(`读取同步状态失败：${String(e)}`);
  } finally {
    loading.value = false;
  }
}

// ---- 开启：设口令（口令是唯一根，忘了且无已配对设备 = 不可恢复，文案如实说） ----
const showEnable = ref(false);
const passphrase = ref('');
const passphrase2 = ref('');
const enabling = ref(false);

const strengthHint = computed(() => {
  const n = passphrase.value.trim().length;
  if (n === 0) return '';
  if (n < 8) return '至少 8 位';
  if (n < 12) return '强度：偏弱';
  if (n < 16) return '强度：良好';
  return '强度：很强';
});

async function enable() {
  if (!isTauri() || enabling.value) return;
  if (passphrase.value.trim().length < 8) {
    showToast('口令至少 8 位');
    return;
  }
  if (passphrase.value !== passphrase2.value) {
    showToast('两次输入的口令不一致');
    return;
  }
  enabling.value = true;
  try {
    await tauriApi.syncEnable(passphrase.value);
    passphrase.value = '';
    passphrase2.value = '';
    showEnable.value = false;
    showToast('云同步已开启');
    await loadStatus();
  } catch (e) {
    showToast(`开启失败：${String(e)}`);
  } finally {
    enabling.value = false;
  }
}

// ---- 关闭：两段式确认（云端密文保留，本机吊销设备行并清钥匙） ----
const confirmOff = ref(false);
let confirmOffTimer: ReturnType<typeof setTimeout> | null = null;

async function toggleSync() {
  if (!isTauri() || !status.value) return;
  if (!status.value.paired) {
    showEnable.value = !showEnable.value;
    return;
  }
  // 已开启 → 两段式关闭
  if (!confirmOff.value) {
    confirmOff.value = true;
    if (confirmOffTimer) clearTimeout(confirmOffTimer);
    confirmOffTimer = setTimeout(() => {
      confirmOff.value = false;
    }, 3000);
    return;
  }
  confirmOff.value = false;
  try {
    await tauriApi.syncDisable();
    stopPairingUi(); // 关闭时顺带停掉可能开着的配对 UI
    showToast('已关闭本机同步（云端密文保留）');
    await loadStatus();
  } catch (e) {
    showToast(`关闭失败：${String(e)}`);
  }
}

// ---- 立即同步 ----
const syncing = ref(false);

async function syncNow() {
  if (!isTauri() || syncing.value) return;
  syncing.value = true;
  try {
    const r = await tauriApi.syncNow();
    lastSyncAt.value = Date.now();
    localStorage.setItem(LAST_SYNC_KEY, String(lastSyncAt.value));
    const rejected = r.rejected ?? 0;
    // failed = 拉取时解不开/应用不了的记录数 + 推送被服务端拒绝的条目数。
    // 只报「已是最新」会让静默失败看起来一切正常（比如密钥轮换后永远解不开），必须说出来
    if (r.failed > 0) {
      showToast(
        `同步完成但有 ${r.failed} 条未处理（收到 ${r.applied} 条，推送 ${r.pushed} 条）` +
          '——可能口令与云端数据不匹配，或记录冲突，稍后重试仍失败请联系支持',
      )
    } else if (rejected > 0) {
      // 服务端按条拒收（多半是单条数据超过上限）：其余条目照常同步成功，
      // 但要让用户知道有几条没上去，而不是看着「同步完成」以为全都好了
      showToast(
        `已同步，但有 ${rejected} 条被服务端拒收（多为单条内容过大）` +
          `——收到 ${r.applied} 条，推送 ${r.pushed} 条`,
      )
    } else {
      showToast(
        r.applied + r.pushed > 0
          ? `同步完成：收到 ${r.applied} 条，推送 ${r.pushed} 条`
          : '已是最新',
      )
    }
    await loadStatus();
  } catch (e) {
    showToast(`同步失败：${String(e)}`);
  } finally {
    syncing.value = false;
  }
}

// ---- 自动同步开关（默认关）：开 = 本机改动 / 定时 / 聚焦自动同步；关 = 只手动「立即同步」 ----
const autoSync = computed(() => store.state.config.sync_auto_enabled === true);

async function toggleAutoSync() {
  const next = !autoSync.value;
  await store.setSyncAutoEnabled(next);
  showToast(next ? '已开启自动同步' : '已关闭自动同步，同步请点「立即同步」');
}

// ---- 配对二维码（TTL 300s，一次性；轮询 2s，完成即收） ----
const pairingActive = ref(false);
const qrDataUrl = ref('');
const pairingStatus = ref<'' | 'waiting' | 'completed' | 'expired'>('');
const countdown = ref(0);
let pollTimer: ReturnType<typeof setInterval> | null = null;
let tickTimer: ReturnType<typeof setInterval> | null = null;
let pairingId = '';

async function beginPairing() {
  if (!isTauri() || pairingActive.value) return;
  try {
    const qr = await tauriApi.syncBeginPairing();
    pairingId = JSON.parse(qr).pid as string;
    qrDataUrl.value = await QRCode.toDataURL(qr, {
      width: 176,
      margin: 1,
      color: { dark: '#1f2937', light: '#ffffff' },
    });
    pairingActive.value = true;
    pairingStatus.value = 'waiting';
    countdown.value = 300;
    startPolling();
  } catch (e) {
    showToast(`发起配对失败：${String(e)}`);
  }
}

function startPolling() {
  stopPolling();
  pollTimer = setInterval(async () => {
    if (!pairingId) return;
    try {
      const r = await tauriApi.syncPollPairing(pairingId);
      if (r.status === 'completed') {
        pairingStatus.value = 'completed';
        stopPairingUi();
        showToast(`手机「${r.deviceName || '新设备'}」已加入同步`);
        await loadStatus();
      } else if (r.status === 'expired' || r.status === 'cancelled') {
        pairingStatus.value = 'expired';
        stopPairingUi();
      }
    } catch {
      // 单次轮询失败不打断（网络抖动）；过期由倒计时兜底
    }
  }, 2000);
  tickTimer = setInterval(() => {
    countdown.value--;
    if (countdown.value <= 0) {
      pairingStatus.value = 'expired';
      stopPairingUi();
    }
  }, 1000);
}

function stopPolling() {
  if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  if (tickTimer) { clearInterval(tickTimer); tickTimer = null; }
}

/** 收起二维码 UI；会话还挂着就顺手取消（一次性，不给拍照留时间窗） */
async function stopPairingUi() {
  stopPolling();
  const hadSession = pairingActive.value;
  pairingActive.value = false;
  qrDataUrl.value = '';
  pairingStatus.value = '';
  if (hadSession && pairingId && isTauri()) {
    void tauriApi.syncPairingCancel(pairingId).catch(() => {});
  }
  pairingId = '';
}

// ---- 设备列表 ----
const devices = ref<SyncDeviceInfo[]>([]);
const revokingId = ref('');
let revokeTimer: ReturnType<typeof setTimeout> | null = null;

async function loadDevices() {
  if (!isTauri()) return;
  try {
    devices.value = (await tauriApi.syncDeviceList()).devices;
  } catch {
    // 未开启/网络失败时静默（首屏 status 已有报错通道）
  }
}

function kindLabel(kind: string): string {
  return kind === 'desktop' ? '桌面' : kind === 'mini' ? '手机' : kind;
}

function seenLabel(ts: number | null): string {
  if (!ts) return '从未连接';
  const diff = Date.now() - ts;
  if (diff < 60_000) return '刚刚在线';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前在线`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前在线`;
  return `${Math.floor(diff / 86_400_000)} 天前在线`;
}

async function revokeDevice(d: SyncDeviceInfo) {
  if (!isTauri()) return;
  // 两段式：第一下标记确认，3 秒后自动取消
  if (revokingId.value !== d.id) {
    revokingId.value = d.id;
    if (revokeTimer) clearTimeout(revokeTimer);
    revokeTimer = setTimeout(() => {
      revokingId.value = '';
    }, 3000);
    return;
  }
  revokingId.value = '';
  try {
    await tauriApi.syncRevokeDevice(d.id);
    showToast(`已撤销「${d.name || kindLabel(d.kind)}」，该设备需重新扫码加入`);
    await loadDevices();
  } catch (e) {
    showToast(`撤销失败：${String(e)}`);
  }
}

function fmtLastSync(): string {
  if (!lastSyncAt.value) return '从未同步';
  const diff = Date.now() - lastSyncAt.value;
  if (diff < 60_000) return '刚刚';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return `${Math.floor(diff / 86_400_000)} 天前`;
}

onMounted(() => {
  void loadStatus();
});

onBeforeUnmount(() => {
  // 二维码是一次性凭据：离开面板就收掉，不留拍照窗口
  void stopPairingUi();
  if (confirmOffTimer) clearTimeout(confirmOffTimer);
  if (revokeTimer) clearTimeout(revokeTimer);
});
</script>

<template>
  <section id="sv-sec-sync" class="sv-sec" aria-label="云同步">
    <h3 class="sv-sec-title">云同步</h3>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">云同步</span>
        <span class="setting-desc">
          速记与待办端到端加密后经云中枢在桌面与手机间同步；服务端只存密文，解不开内容
        </span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="!!status?.paired"
        :class="{ on: !!status?.paired }"
        :disabled="loading"
        @click="toggleSync"
      >
        <span class="toggle-knob"></span>
      </button>
    </div>

    <!-- 未开启：口令录入（身份与密钥分离的第一步——口令只在本机派生） -->
    <div v-if="showEnable && !status?.paired" class="sync-enable">
      <p class="sync-hint warn">
        设置同步口令（至少 8 位）。口令是唯一的解密根，只在本机派生、不经任何传输；
        <strong>忘记口令且没有任何已配对设备时，云端数据不可恢复</strong>。
      </p>
      <div class="sync-field">
        <input
          v-model="passphrase"
          class="sync-input"
          type="password"
          placeholder="同步口令"
          autocomplete="new-password"
          @keydown.enter="enable"
        />
        <span v-if="strengthHint" class="sync-strength">{{ strengthHint }}</span>
      </div>
      <div class="sync-field">
        <input
          v-model="passphrase2"
          class="sync-input"
          type="password"
          placeholder="再输一遍"
          autocomplete="new-password"
          @keydown.enter="enable"
        />
      </div>
      <div class="sync-actions">
        <button class="ghost-btn" type="button" @click="showEnable = false">取消</button>
        <button class="pill-btn" type="button" :disabled="enabling" @click="enable">
          {{ enabling ? '正在派生密钥…' : '开启云同步' }}
        </button>
      </div>
    </div>

    <!-- 已开启：状态 + 配对 + 设备 -->
    <template v-if="status?.paired">
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-name">自动同步</span>
          <span class="setting-desc">
            开启后本机改动与远端变更自动同步（后台节流 20 秒，不打扰使用）；
            关闭则只在点下方「立即同步」时同步
          </span>
        </div>
        <button
          class="toggle"
          role="switch"
          type="button"
          :aria-checked="autoSync"
          :class="{ on: autoSync }"
          @click="toggleAutoSync"
        >
          <span class="toggle-knob"></span>
        </button>
      </div>

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-name">同步空间</span>
          <span class="setting-desc">
            空间 {{ status.spaceId }} · 上次同步 {{ fmtLastSync() }}
            <template v-if="status.pending > 0"> · {{ status.pending }} 条待推送</template>
          </span>
        </div>
        <button class="ghost-btn data-btn" :disabled="syncing" @click="syncNow">
          <RefreshCw :size="14" :stroke-width="2" :class="{ spinning: syncing }" />
          {{ syncing ? '同步中…' : '立即同步' }}
        </button>
      </div>

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-name">手机配对</span>
          <span class="setting-desc">手机 x-hub 小程序扫码加入本空间；二维码只含一次性配对凭据，5 分钟有效</span>
        </div>
        <button
          v-if="!pairingActive"
          class="ghost-btn data-btn"
          @click="beginPairing"
        >
          <QrCodeIcon :size="14" :stroke-width="2" />
          显示配对二维码
        </button>
        <button v-else class="ghost-btn data-btn" @click="stopPairingUi">
          <X :size="14" :stroke-width="2" />
          收起
        </button>
      </div>

      <!-- 二维码 + 等待态 -->
      <div v-if="pairingActive" class="sync-qr-card">
        <img v-if="qrDataUrl" class="sync-qr" :src="qrDataUrl" alt="配对二维码" />
        <p v-if="pairingStatus === 'waiting'" class="sync-hint">
          用手机小程序「云同步 → 扫码配对」扫描，{{ Math.floor(countdown / 60) }}:{{ String(countdown % 60).padStart(2, '0') }} 后过期
        </p>
        <p v-else-if="pairingStatus === 'expired'" class="sync-hint warn">二维码已过期，请重新生成</p>
      </div>

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-name">同步设备</span>
          <span class="setting-desc">免费额度 1 桌面 + 1 手机；撤销后该设备需重新扫码，且没有口令就读不到内容</span>
        </div>
      </div>
      <div v-if="devices.length" class="sync-devices">
        <div v-for="d in devices" :key="d.id" class="sync-device">
          <component :is="d.kind === 'mini' ? Smartphone : Monitor" :size="14" :stroke-width="2" class="sync-device-icon" />
          <span class="sync-device-name">{{ d.name || '未命名设备' }}</span>
          <span class="sync-device-meta">{{ kindLabel(d.kind) }} · {{ seenLabel(d.last_seen_at) }}</span>
          <button
            class="sync-device-revoke"
            :class="{ confirm: revokingId === d.id }"
            type="button"
            @click="revokeDevice(d)"
          >
            {{ revokingId === d.id ? '确认撤销？' : '撤销' }}
          </button>
        </div>
      </div>

      <p class="settings-foot sync-foot">
        <Lock :size="12" :stroke-width="2" class="settings-lock" aria-hidden="true" />
        端到端加密：口令与密钥只存在于你的设备上；换口令需全量重加密重传（当前版本暂不支持更换）
      </p>
    </template>
  </section>
</template>

<style scoped>
/* 开启表单 */
.sync-enable {
  margin: 4px 0 10px;
  padding: 12px 14px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.sync-hint {
  margin: 0 0 10px;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-2);
}
.sync-hint.warn {
  color: var(--warning, #b45309);
}
.sync-field {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}
.sync-input {
  flex: 1;
  max-width: 280px;
  padding: 6px 10px;
  font-size: 0.8125rem;
  color: var(--text-1);
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm, 6px);
  outline: none;
}
.sync-input:focus {
  border-color: var(--accent, var(--text-3));
}
.sync-strength {
  font-size: 0.6875rem;
  color: var(--text-3);
  white-space: nowrap;
}
.sync-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

/* 二维码卡片 */
.sync-qr-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  margin: 4px 0 12px;
  padding: 14px;
  border: 1px dashed var(--border-soft);
  border-radius: var(--radius-md);
}
.sync-qr {
  width: 176px;
  height: 176px;
  border-radius: 8px;
  background: #fff;
}

/* 设备列表 */
.sync-devices {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0 0 12px;
}
.sync-device {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.sync-device-icon {
  flex: none;
  color: var(--text-3);
}
.sync-device-name {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
}
.sync-device-meta {
  flex: 1;
  font-size: 0.6875rem;
  color: var(--text-3);
}
.sync-device-revoke {
  padding: 3px 10px;
  font-size: 0.6875rem;
  color: var(--text-2);
  background: transparent;
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  cursor: pointer;
}
.sync-device-revoke:hover {
  color: var(--text-1);
  border-color: var(--text-3);
}
.sync-device-revoke.confirm {
  color: #b91c1c;
  border-color: #b91c1c;
}

/* 同步中图标旋转 */
.spinning {
  animation: sync-spin 0.9s linear infinite;
}
@keyframes sync-spin {
  to {
    transform: rotate(360deg);
  }
}

.sync-foot {
  margin-top: 2px;
}
</style>
