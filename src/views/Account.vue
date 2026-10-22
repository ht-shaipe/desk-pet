<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { ArrowRight, CircleCloseFilled, Loading, UserFilled } from "@element-plus/icons-vue";
import { useConfigStore } from "@/stores/config";
import {
  ApiError,
  extractToken,
  getCaptcha,
  getSlider,
  login,
  mobileExists,
  register,
  verifyConfig,
  type CaptchaData,
  type SliderData,
  type VerifyConfig,
  type VerifyMode,
} from "@/api/auth";

const { t } = useI18n();
const config = useConfigStore();

type Mode = "login" | "register";

const mode = ref<Mode>("login");
const mobile = ref("");
const password = ref("");
const confirmPassword = ref("");
const loading = ref(false);
const errorMsg = ref("");

const verifyCfg = ref<VerifyConfig | null>(null);
const captcha = ref<CaptchaData | null>(null);
const captchaCode = ref("");
const slider = ref<SliderData | null>(null);
const sliderX = ref(0);
const sliderDragging = ref(false);
const sliderDone = ref(false);
const sliderScale = ref(1);

const loggedIn = computed(() => !!config.settings.auth_token);
const authUser = computed(() => config.settings.auth_user ?? "");
const maskedUser = computed(() =>
  authUser.value.replace(/^(\d{3})\d{4}(\d{4})$/, "$1****$2"),
);

const currentVerifyMode = computed<VerifyMode>(() => {
  if (!verifyCfg.value) {
    return "none";
  }
  return mode.value === "login" ? verifyCfg.value.login : verifyCfg.value.register;
});

/** 滑块可视宽度适配容器（面板内容区约 340px）。 */
const sliderBoxStyle = computed(() => {
  const s = slider.value;
  if (!s) {
    return {};
  }
  return {
    width: `${Math.round(s.width * sliderScale.value)}px`,
    height: `${Math.round(44 * sliderScale.value)}px`,
  };
});

function sliderImageSrc(image: string): string {
  return image.startsWith("data:") ? image : `data:image/png;base64,${image}`;
}

async function refreshCaptcha() {
  captcha.value = null;
  captchaCode.value = "";
  try {
    captcha.value = await getCaptcha();
  } catch (e) {
    errorMsg.value = String(e);
  }
}

async function refreshSlider() {
  slider.value = null;
  sliderX.value = 0;
  sliderDone.value = false;
  try {
    slider.value = await getSlider();
    await nextTick();
    fitSliderScale();
  } catch (e) {
    errorMsg.value = String(e);
  }
}

function fitSliderScale() {
  const s = slider.value;
  if (!s) {
    return;
  }
  const box = sliderWrap.value;
  const available = box && box.clientWidth > 0 ? box.clientWidth : 320;
  sliderScale.value = s.width > available ? available / s.width : 1;
}

const sliderWrap = ref<HTMLElement | null>(null);

async function prepareVerify() {
  errorMsg.value = "";
  if (currentVerifyMode.value === "captcha") {
    await refreshCaptcha();
  } else if (currentVerifyMode.value === "slider") {
    await refreshSlider();
  }
}

async function switchMode(value: Mode) {
  mode.value = value;
  errorMsg.value = "";
  await prepareVerify();
}

function onSliderPointerDown(e: PointerEvent) {
  if (sliderDone.value || !slider.value) {
    return;
  }
  sliderDragging.value = true;
  (e.target as HTMLElement).setPointerCapture(e.pointerId);
}

function onSliderPointerMove(e: PointerEvent) {
  const s = slider.value;
  if (!sliderDragging.value || !s) {
    return;
  }
  const rect = sliderWrap.value?.getBoundingClientRect();
  const scale = sliderScale.value;
  const localX = rect ? (e.clientX - rect.left) / scale : 0;
  const maxX = s.width - s.sliderWidth;
  sliderX.value = Math.min(maxX, Math.max(0, localX - s.sliderWidth / 2));
}

function onSliderPointerUp() {
  const s = slider.value;
  if (!sliderDragging.value || !s) {
    return;
  }
  sliderDragging.value = false;
  if (Math.abs(sliderX.value - s.targetX) <= 3) {
    sliderDone.value = true;
    sliderX.value = s.targetX;
    ElMessage.success(t("account.sliderDone"));
  } else {
    ElMessage.warning(t("account.sliderMiss"));
  }
}

function buildPayload() {
  const payload: {
    mobile: string;
    password: string;
    ticket?: string;
    code?: string;
    x?: number;
  } = { mobile: mobile.value.trim(), password: password.value };
  if (currentVerifyMode.value === "captcha" && captcha.value) {
    payload.ticket = captcha.value.ticket;
    payload.code = captchaCode.value.trim();
  } else if (currentVerifyMode.value === "slider" && slider.value) {
    payload.ticket = slider.value.ticket;
    payload.x = Math.round(sliderX.value);
  }
  return payload;
}

function validate(): string {
  if (!/^1\d{10}$/.test(mobile.value.trim())) {
    return t("account.badMobile");
  }
  if (password.value.length < 6) {
    return t("account.badPassword");
  }
  if (mode.value === "register" && password.value !== confirmPassword.value) {
    return t("account.passwordMismatch");
  }
  if (currentVerifyMode.value === "captcha" && !captchaCode.value.trim()) {
    return t("account.needCaptcha");
  }
  if (currentVerifyMode.value === "slider" && !sliderDone.value) {
    return t("account.needSlider");
  }
  return "";
}

async function submit() {
  const invalid = validate();
  if (invalid) {
    errorMsg.value = invalid;
    return;
  }
  errorMsg.value = "";
  loading.value = true;
  try {
    if (mode.value === "register") {
      const exists = await mobileExists(mobile.value.trim());
      if (exists) {
        errorMsg.value = t("account.mobileTaken");
        loading.value = false;
        return;
      }
    }
    const payload = buildPayload();
    const result = mode.value === "login" ? await login(payload) : await register(payload);
    const token = extractToken(result);
    if (!token) {
      errorMsg.value = t("account.noToken");
      await prepareVerify();
      return;
    }
    await config.setAuth(token, mobile.value.trim());
    ElMessage.success(mode.value === "login" ? t("account.loginOk") : t("account.registerOk"));
  } catch (e) {
    const msg = e instanceof ApiError ? e.message : String(e);
    errorMsg.value = msg;
    // 4013 = 验证失败/过期，刷新验证码或滑块让用户重试
    if (e instanceof ApiError && e.code === 4013) {
      await prepareVerify();
    }
  } finally {
    loading.value = false;
  }
}

async function onLogout() {
  await config.logout();
  mode.value = "login";
  mobile.value = "";
  password.value = "";
  confirmPassword.value = "";
  captchaCode.value = "";
  await prepareVerify();
  ElMessage.success(t("account.loggedOut"));
}

onMounted(async () => {
  try {
    verifyCfg.value = await verifyConfig();
  } catch (e) {
    errorMsg.value = t("account.networkError", { err: String(e) });
    return;
  }
  if (!loggedIn.value) {
    await prepareVerify();
  }
});
</script>

<template>
  <div class="account-view">
    <!-- 已登录 -->
    <div v-if="loggedIn" class="logged-card">
      <div class="logged-head">
        <div class="avatar"><el-icon :size="30"><UserFilled /></el-icon></div>
        <div class="user-name">{{ maskedUser }}</div>
      </div>
      <div class="logged-body">
        <div class="user-hint">{{ t("account.syncHint") }}</div>
        <el-button size="small" class="logout-btn" @click="onLogout">
          {{ t("account.logout") }}
        </el-button>
      </div>
    </div>

    <!-- 未登录 -->
    <div v-else class="brand-card">
      <div class="brand-head">
        <img class="brand-logo" src="/images/avatar.png" alt="logo" draggable="false" />
        <div class="brand-title">Desk Pet</div>
        <div class="brand-sub">{{ mode === "login" ? t("account.welcomeBack") : t("account.createAccount") }}</div>
      </div>

      <div class="brand-body">
        <div class="seg-tabs">
          <button
            class="seg-tab"
            :class="{ active: mode === 'login' }"
            @click="switchMode('login')"
          >
            {{ t("account.login") }}
          </button>
          <button
            class="seg-tab"
            :class="{ active: mode === 'register' }"
            @click="switchMode('register')"
          >
            {{ t("account.register") }}
          </button>
        </div>

        <div class="form">
          <el-input
            v-model="mobile"
            size="large"
            :placeholder="t('account.mobilePh')"
            maxlength="11"
            clearable
            inputmode="numeric"
          >
            <template #prefix><el-icon><UserFilled /></el-icon></template>
          </el-input>
          <el-input
            v-model="password"
            size="large"
            type="password"
            :placeholder="t('account.passwordPh')"
            show-password
          />
          <el-input
            v-if="mode === 'register'"
            v-model="confirmPassword"
            size="large"
            type="password"
            :placeholder="t('account.confirmPh')"
            show-password
          />

          <!-- 图形验证码 -->
          <div v-if="currentVerifyMode === 'captcha'" class="captcha-row">
            <el-input
              v-model="captchaCode"
              size="large"
              :placeholder="t('account.captchaPh')"
              maxlength="4"
              class="captcha-input"
            />
            <img
              v-if="captcha"
              :src="sliderImageSrc(captcha.image)"
              class="captcha-img"
              :title="t('account.refreshCaptcha')"
              @click="refreshCaptcha"
            />
          </div>

          <!-- 滑块验证 -->
          <div v-else-if="currentVerifyMode === 'slider'" ref="sliderWrap" class="slider-wrap">
            <div v-if="slider" class="slider-track" :style="sliderBoxStyle">
              <div
                class="slider-target"
                :style="{
                  left: `${slider.targetX * sliderScale}px`,
                  width: `${slider.sliderWidth * sliderScale}px`,
                }"
              ></div>
              <div class="slider-mask" :style="{ width: `${sliderX * sliderScale}px` }"></div>
              <div
                class="slider-handle"
                :class="{ done: sliderDone }"
                :style="{ left: `${sliderX * sliderScale}px`, width: `${slider.sliderWidth * sliderScale}px` }"
                @pointerdown="onSliderPointerDown"
                @pointermove="onSliderPointerMove"
                @pointerup="onSliderPointerUp"
                @pointercancel="onSliderPointerUp"
              >
                <span v-if="sliderDone" class="slider-icon">✓</span>
                <el-icon v-else class="slider-icon"><ArrowRight /></el-icon>
              </div>
              <div v-if="!sliderDone" class="slider-tip">{{ t("account.sliderHint") }}</div>
            </div>
            <div v-else class="slider-loading">
              <el-icon class="is-loading"><Loading /></el-icon>
              {{ t("account.loadingVerify") }}
            </div>
          </div>

          <transition name="err-fade">
            <div v-if="errorMsg" class="error-msg">
              <el-icon><CircleCloseFilled /></el-icon>
              <span>{{ errorMsg }}</span>
            </div>
          </transition>

          <button class="submit-btn" :disabled="loading" @click="submit">
            <el-icon v-if="loading" class="is-loading"><Loading /></el-icon>
            {{ mode === "login" ? t("account.login") : t("account.register") }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.account-view {
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  padding: 6px 2px 2px;
}

/* ---------- 已登录卡片 ---------- */

.logged-card {
  background: var(--dp-card-bg);
  border: 1px solid var(--dp-card-border);
  border-radius: 14px;
  box-shadow: var(--dp-card-shadow);
  overflow: hidden;
}

.logged-head {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 20px 16px 14px;
  background: linear-gradient(135deg, var(--dp-accent), var(--dp-accent-deep, #2f7de1));
  color: #ffffff;
}

.logged-head .avatar {
  width: 56px;
  height: 56px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.22);
  border: 2px solid rgba(255, 255, 255, 0.65);
  display: flex;
  align-items: center;
  justify-content: center;
  backdrop-filter: blur(4px);
}

.logged-head .user-name {
  margin-top: 8px;
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.5px;
}

.logged-body {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 14px 16px 18px;
}

.user-hint {
  font-size: 12px;
  color: var(--dp-text-muted);
}

.logout-btn {
  width: 100%;
}

/* ---------- 品牌卡片 ---------- */

.brand-card {
  background: var(--dp-card-bg);
  border: 1px solid var(--dp-card-border);
  border-radius: 14px;
  box-shadow: var(--dp-card-shadow);
  overflow: hidden;
}

.brand-head {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 22px 16px 16px;
  background: linear-gradient(160deg, var(--dp-accent) -20%, var(--dp-accent-deep, #2f7de1) 85%);
  color: #ffffff;
}

.brand-logo {
  width: 46px;
  height: 46px;
  border-radius: 50%;
  border: 2px solid rgba(255, 255, 255, 0.7);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  object-fit: cover;
}

.brand-title {
  margin-top: 8px;
  font-size: 15px;
  font-weight: 700;
  letter-spacing: 1px;
}

.brand-sub {
  margin-top: 3px;
  font-size: 12px;
  opacity: 0.85;
}

.brand-body {
  padding: 16px 16px 18px;
}

/* ---------- 分段切换 ---------- */

.seg-tabs {
  display: flex;
  gap: 4px;
  padding: 3px;
  background: var(--dp-btn-bg);
  border-radius: 10px;
  margin-bottom: 14px;
}

.seg-tab {
  flex: 1;
  border: none;
  border-radius: 8px;
  padding: 7px 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--dp-text-muted);
  background: transparent;
  cursor: pointer;
  transition: all 0.2s ease;
}

.seg-tab:hover {
  color: var(--dp-text-primary);
}

.seg-tab.active {
  background: var(--dp-card-bg);
  color: var(--dp-accent);
  font-weight: 600;
  box-shadow: 0 1px 4px rgba(31, 41, 55, 0.12);
}

/* ---------- 表单 ---------- */

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.form :deep(.el-input__wrapper) {
  border-radius: 10px;
}

.captcha-row {
  display: flex;
  gap: 8px;
  align-items: center;
}

.captcha-input {
  flex: 1;
}

.captcha-img {
  height: 40px;
  border-radius: 10px;
  cursor: pointer;
  border: 1px solid var(--dp-card-border);
  transition: transform 0.15s ease;
}

.captcha-img:hover {
  transform: scale(1.03);
}

/* ---------- 滑块验证 ---------- */

.slider-wrap {
  width: 100%;
}

.slider-track {
  position: relative;
  height: 44px;
  border-radius: 10px;
  background: var(--dp-btn-bg);
  border: 1px solid var(--dp-card-border);
  overflow: hidden;
}

.slider-mask {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 0;
  background: linear-gradient(90deg, rgba(64, 158, 255, 0.08), rgba(64, 158, 255, 0.18));
  border-right: 1px solid rgba(64, 158, 255, 0.35);
  pointer-events: none;
}

.slider-target {
  position: absolute;
  top: 0;
  bottom: 0;
  border: 2px dashed var(--dp-accent);
  border-radius: 10px;
  opacity: 0.45;
  box-sizing: border-box;
  pointer-events: none;
}

.slider-tip {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  color: var(--dp-text-muted);
  pointer-events: none;
  user-select: none;
}

.slider-handle {
  position: absolute;
  top: 3px;
  bottom: 3px;
  border-radius: 8px;
  background: linear-gradient(160deg, var(--dp-accent), var(--dp-accent-deep, #2f7de1));
  color: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  user-select: none;
  touch-action: none;
  box-shadow: 0 2px 6px rgba(31, 41, 55, 0.25);
}

.slider-handle:active {
  cursor: grabbing;
}

.slider-handle.done {
  background: var(--el-color-success, #67c23a);
}

.slider-icon {
  font-size: 15px;
  line-height: 1;
}

.slider-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  font-size: 12px;
  color: var(--dp-text-muted);
  padding: 14px 0;
}

/* ---------- 错误提示 ---------- */

.error-msg {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 12px;
  color: var(--el-color-danger, #f56c6c);
  line-height: 1.5;
  background: var(--dp-danger-soft);
  border-radius: 8px;
  padding: 8px 10px;
}

.error-msg .el-icon {
  flex-shrink: 0;
  margin-top: 1px;
}

.err-fade-enter-active,
.err-fade-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.err-fade-enter-from,
.err-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

/* ---------- 提交按钮 ---------- */

.submit-btn {
  width: 100%;
  border: none;
  border-radius: 10px;
  padding: 11px 0;
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 2px;
  color: #ffffff;
  background: linear-gradient(160deg, var(--dp-accent), var(--dp-accent-deep, #2f7de1));
  box-shadow: 0 4px 12px rgba(47, 125, 225, 0.35);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  transition: transform 0.15s ease, box-shadow 0.15s ease, opacity 0.15s ease;
}

.submit-btn:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow: 0 6px 16px rgba(47, 125, 225, 0.45);
}

.submit-btn:active:not(:disabled) {
  transform: translateY(0);
}

.submit-btn:disabled {
  opacity: 0.7;
  cursor: not-allowed;
}
</style>
