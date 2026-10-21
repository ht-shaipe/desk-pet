<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { UserFilled } from "@element-plus/icons-vue";
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
} from "@/services/auth";

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
      <div class="avatar"><el-icon :size="26"><UserFilled /></el-icon></div>
      <div class="user-info">
        <div class="user-name">{{ maskedUser }}</div>
        <div class="user-hint">{{ t("account.syncHint") }}</div>
      </div>
      <el-button size="small" @click="onLogout">{{ t("account.logout") }}</el-button>
    </div>

    <!-- 未登录 -->
    <template v-else>
      <div class="mode-switch">
        <el-radio-group :model-value="mode" @update:model-value="switchMode($event as Mode)">
          <el-radio-button value="login">{{ t("account.login") }}</el-radio-button>
          <el-radio-button value="register">{{ t("account.register") }}</el-radio-button>
        </el-radio-group>
      </div>

      <div class="form">
        <el-input v-model="mobile" :placeholder="t('account.mobilePh')" maxlength="11" clearable>
          <template #prefix><el-icon><UserFilled /></el-icon></template>
        </el-input>
        <el-input
          v-model="password"
          type="password"
          :placeholder="t('account.passwordPh')"
          show-password
        />
        <el-input
          v-if="mode === 'register'"
          v-model="confirmPassword"
          type="password"
          :placeholder="t('account.confirmPh')"
          show-password
        />

        <!-- 图形验证码 -->
        <div v-if="currentVerifyMode === 'captcha'" class="captcha-row">
          <el-input
            v-model="captchaCode"
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
            <div
              class="slider-handle"
              :class="{ done: sliderDone }"
              :style="{ left: `${sliderX * sliderScale}px`, width: `${slider.sliderWidth * sliderScale}px` }"
              @pointerdown="onSliderPointerDown"
              @pointermove="onSliderPointerMove"
              @pointerup="onSliderPointerUp"
              @pointercancel="onSliderPointerUp"
            >
              {{ sliderDone ? "✓" : "→" }}
            </div>
          </div>
          <div v-else class="slider-loading">{{ t("account.loadingVerify") }}</div>
        </div>

        <div v-if="errorMsg" class="error-msg">{{ errorMsg }}</div>

        <el-button type="primary" :loading="loading" class="submit-btn" @click="submit">
          {{ mode === "login" ? t("account.login") : t("account.register") }}
        </el-button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.account-view {
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
  padding: 4px 2px;
}

.logged-card {
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--dp-card-bg);
  border: 1px solid var(--dp-card-border);
  border-radius: 12px;
  padding: 14px;
  box-shadow: var(--dp-card-shadow);
}

.avatar {
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: var(--dp-accent);
  color: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.user-info {
  flex: 1;
  min-width: 0;
}

.user-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--dp-text-primary);
}

.user-hint {
  font-size: 12px;
  color: var(--dp-text-muted);
  margin-top: 2px;
}

.mode-switch {
  display: flex;
  justify-content: center;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
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
  height: 32px;
  border-radius: 6px;
  cursor: pointer;
  border: 1px solid var(--dp-card-border);
}

.slider-wrap {
  width: 100%;
}

.slider-track {
  position: relative;
  height: 44px;
  border-radius: 8px;
  background: var(--dp-btn-bg);
  border: 1px solid var(--dp-card-border);
  overflow: hidden;
}

.slider-target {
  position: absolute;
  top: 0;
  bottom: 0;
  border: 2px dashed var(--dp-accent);
  border-radius: 8px;
  opacity: 0.5;
  box-sizing: border-box;
  pointer-events: none;
}

.slider-handle {
  position: absolute;
  top: 3px;
  bottom: 3px;
  border-radius: 6px;
  background: var(--dp-accent);
  color: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  user-select: none;
  touch-action: none;
  font-size: 14px;
}

.slider-handle:active {
  cursor: grabbing;
}

.slider-handle.done {
  background: var(--el-color-success, #67c23a);
}

.slider-loading {
  font-size: 12px;
  color: var(--dp-text-muted);
  text-align: center;
  padding: 12px 0;
}

.error-msg {
  font-size: 12px;
  color: var(--el-color-danger, #f56c6c);
  line-height: 1.4;
}

.submit-btn {
  width: 100%;
}
</style>
