<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage, ElMessageBox } from "element-plus";
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
  try {
    await ElMessageBox.confirm(t("account.logoutConfirm"), t("account.logout"), {
      confirmButtonText: t("account.logout"),
      cancelButtonText: t("common.cancel"),
      type: "warning",
    });
  } catch {
    return;
  }
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
  <div class="view-root">
    <el-scrollbar class="view-scroll">
      <div class="pt-1.5 px-0.5 pb-0.5">
    <!-- 已登录 -->
    <div v-if="loggedIn" class="bg-dp-card-bg border border-dp-card-border rounded-[14px] shadow-dp-card overflow-hidden">
      <div
        class="flex flex-col items-center pt-5 px-4 pb-3.5 text-white bg-[linear-gradient(135deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))]"
      >
        <div
          class="w-14 h-14 rounded-full bg-[rgba(255,255,255,0.22)] border-2 border-[rgba(255,255,255,0.65)] flex items-center justify-center backdrop-blur-[4px]"
        >
          <el-icon :size="30"><UserFilled /></el-icon>
        </div>
        <div class="mt-2 text-[15px] font-semibold tracking-[0.5px]">{{ maskedUser }}</div>
      </div>
      <div class="flex flex-col items-center gap-2.5 pt-3.5 px-4 pb-4.5">
        <div class="text-xs text-dp-muted">{{ t("account.syncHint") }}</div>
        <el-button class="w-full h-[42px] p-0 rounded-[10px] text-sm font-semibold" @click="onLogout">
          {{ t("account.logout") }}
        </el-button>
      </div>
    </div>

    <!-- 未登录 -->
    <div v-else class="bg-dp-card-bg border border-dp-card-border rounded-[14px] shadow-dp-card overflow-hidden">
      <div
        class="flex flex-col items-center pt-5.5 px-4 pb-4 text-white bg-[linear-gradient(160deg,var(--dp-accent)_-20%,var(--dp-accent-deep,#2f7de1)_85%)]"
      >
        <img
          class="w-[46px] h-[46px] rounded-full border-2 border-[rgba(255,255,255,0.7)] shadow-[0_4px_12px_rgba(0,0,0,0.2)] object-cover"
          src="/images/avatar.png"
          alt="logo"
          draggable="false"
        />
        <div class="mt-2 text-[15px] font-bold tracking-[1px]">{{ config.settings.app_name ?? "Desk Pet" }}</div>
        <div class="mt-[3px] text-xs opacity-85">{{ mode === "login" ? t("account.welcomeBack") : t("account.createAccount") }}</div>
      </div>

      <div class="px-4 pt-4 pb-4.5">
        <div class="flex gap-1 p-[3px] bg-dp-btn-bg rounded-[10px] mb-3.5">
          <button
            class="flex-1 rounded-lg py-[7px] text-[13px] cursor-pointer transition-all duration-200 ease-[ease]"
            :class="
              mode === 'login'
                ? 'bg-dp-card-bg text-dp-accent font-semibold shadow-[0_1px_4px_rgba(31,41,55,0.12)]'
                : 'bg-transparent text-dp-muted font-medium hover:text-dp-primary'
            "
            @click="switchMode('login')"
          >
            {{ t("account.login") }}
          </button>
          <button
            class="flex-1 rounded-lg py-[7px] text-[13px] cursor-pointer transition-all duration-200 ease-[ease]"
            :class="
              mode === 'register'
                ? 'bg-dp-card-bg text-dp-accent font-semibold shadow-[0_1px_4px_rgba(31,41,55,0.12)]'
                : 'bg-transparent text-dp-muted font-medium hover:text-dp-primary'
            "
            @click="switchMode('register')"
          >
            {{ t("account.register") }}
          </button>
        </div>

        <div class="flex flex-col gap-3 [&_.el-input\_\_wrapper]:rounded-[10px]">
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
          <div v-if="currentVerifyMode === 'captcha'" class="flex gap-2 items-center">
            <el-input
              v-model="captchaCode"
              size="large"
              :placeholder="t('account.captchaPh')"
              maxlength="4"
              class="flex-1"
            />
            <img
              v-if="captcha"
              :src="sliderImageSrc(captcha.image)"
              class="h-10 rounded-[10px] cursor-pointer border border-dp-card-border transition-transform duration-150 ease-[ease] hover:scale-[1.03]"
              :title="t('account.refreshCaptcha')"
              @click="refreshCaptcha"
            />
          </div>

          <!-- 滑块验证 -->
          <div v-else-if="currentVerifyMode === 'slider'" ref="sliderWrap" class="w-full">
            <div
              v-if="slider"
              class="relative h-11 rounded-[10px] bg-dp-btn-bg border border-dp-card-border overflow-hidden"
              :style="sliderBoxStyle"
            >
              <div
                class="absolute top-0 bottom-0 border-2 border-dashed border-dp-accent rounded-[10px] opacity-45 box-border pointer-events-none"
                :style="{
                  left: `${slider.targetX * sliderScale}px`,
                  width: `${slider.sliderWidth * sliderScale}px`,
                }"
              ></div>
              <div
                class="absolute top-0 bottom-0 left-0 bg-[linear-gradient(90deg,rgba(64,158,255,0.08),rgba(64,158,255,0.18))] border-r border-[rgba(64,158,255,0.35)] pointer-events-none"
                :style="{ width: `${sliderX * sliderScale}px` }"
              ></div>
              <div
                class="absolute top-[3px] bottom-[3px] rounded-lg text-white flex items-center justify-center cursor-grab active:cursor-grabbing select-none touch-none shadow-[0_2px_6px_rgba(31,41,55,0.25)]"
                :class="
                  sliderDone
                    ? 'bg-[var(--el-color-success,#67c23a)]'
                    : 'bg-[linear-gradient(160deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))]'
                "
                :style="{ left: `${sliderX * sliderScale}px`, width: `${slider.sliderWidth * sliderScale}px` }"
                @pointerdown="onSliderPointerDown"
                @pointermove="onSliderPointerMove"
                @pointerup="onSliderPointerUp"
                @pointercancel="onSliderPointerUp"
              >
                <span v-if="sliderDone" class="text-[15px] leading-none">✓</span>
                <el-icon v-else class="text-[15px] leading-none"><ArrowRight /></el-icon>
              </div>
              <div
                v-if="!sliderDone"
                class="absolute inset-0 flex items-center justify-center text-xs text-dp-muted pointer-events-none select-none"
              >
                {{ t("account.sliderHint") }}
              </div>
            </div>
            <div v-else class="flex items-center justify-center gap-1.5 text-xs text-dp-muted py-3.5">
              <el-icon class="is-loading"><Loading /></el-icon>
              {{ t("account.loadingVerify") }}
            </div>
          </div>

          <transition
            enter-active-class="transition-[opacity,transform] duration-200 ease-[ease]"
            leave-active-class="transition-[opacity,transform] duration-200 ease-[ease]"
            enter-from-class="opacity-0 -translate-y-1"
            leave-to-class="opacity-0 -translate-y-1"
          >
            <div
              v-if="errorMsg"
              class="flex items-start gap-1.5 text-xs text-[var(--el-color-danger,#f56c6c)] leading-[1.5] bg-dp-danger-soft rounded-lg px-2.5 py-2 [&_.el-icon]:shrink-0 [&_.el-icon]:mt-[1px]"
            >
              <el-icon><CircleCloseFilled /></el-icon>
              <span>{{ errorMsg }}</span>
            </div>
          </transition>

          <el-button
            type="primary"
            class="w-full h-[42px] p-0 border-transparent rounded-[10px] text-sm font-semibold tracking-[2px] text-white bg-[linear-gradient(160deg,var(--dp-accent),var(--dp-accent-deep,#2f7de1))] shadow-[0_4px_12px_rgba(47,125,225,0.35)] transition-[transform,box-shadow,opacity] duration-150 ease-[ease] hover:shadow-[0_6px_16px_rgba(47,125,225,0.45)] hover:-translate-y-[1px] hover:text-white focus-visible:text-white focus-visible:shadow-[0_6px_16px_rgba(47,125,225,0.45)] focus-visible:-translate-y-[1px] active:translate-y-0 disabled:opacity-70 disabled:shadow-none disabled:transform-none disabled:border-transparent disabled:text-white hover:disabled:shadow-none hover:disabled:transform-none [&.is-loading]:before:hidden"
            :loading="loading"
            @click="submit"
          >
            {{ mode === "login" ? t("account.login") : t("account.register") }}
          </el-button>
        </div>
      </div>
    </div>
      </div>
    </el-scrollbar>
  </div>
</template>

