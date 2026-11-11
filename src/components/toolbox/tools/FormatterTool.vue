<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { ElMessage } from "element-plus";
import { Delete, DocumentCopy, MagicStick } from "@element-plus/icons-vue";
import { copyToClipboard } from "@/api/clipboard";
import * as prettier from "prettier/standalone";
import * as pluginBabel from "prettier/plugins/babel";
import * as pluginEstree from "prettier/plugins/estree";
import * as pluginTypescript from "prettier/plugins/typescript";
import * as pluginHtml from "prettier/plugins/html";
import * as pluginPostcss from "prettier/plugins/postcss";
import * as pluginYaml from "prettier/plugins/yaml";

const { t } = useI18n();

/** auto 为自动检测，其余值即 prettier parser 名。 */
type FormatLang =
  | "auto"
  | "json"
  | "babel"
  | "typescript"
  | "html"
  | "vue"
  | "css"
  | "scss"
  | "less"
  | "yaml";
type IndentMode = "2" | "4" | "tab";

/** 非通用命名的语言展示名（JSON/HTML 等直接沿用 option label）。 */
const LANG_LABELS: Record<Exclude<FormatLang, "auto">, string> = {
  json: "JSON",
  babel: "JavaScript",
  typescript: "TypeScript",
  html: "HTML",
  vue: "Vue",
  css: "CSS",
  scss: "SCSS",
  less: "LESS",
  yaml: "YAML",
};

const PRETTIER_PLUGINS = [
  pluginBabel,
  pluginEstree,
  pluginTypescript,
  pluginHtml,
  pluginPostcss,
  pluginYaml,
];

const language = ref<FormatLang>("auto");
const indent = ref<IndentMode>("2");

const input = ref("");
const output = ref("");
const detectedLang = ref<Exclude<FormatLang, "auto"> | "">("");
const errorMsg = ref("");
const formatting = ref(false);

const langOptions = computed(() => {
  const auto = { value: "auto" as const, label: t("toolbox.tools.formatter.auto") };
  return [auto, ...Object.entries(LANG_LABELS).map(([value, label]) => ({ value: value as FormatLang, label }))];
});

const indentOptions = computed(() => [
  { value: "2" as IndentMode, label: t("toolbox.tools.formatter.indent2") },
  { value: "4" as IndentMode, label: t("toolbox.tools.formatter.indent4") },
  { value: "tab" as IndentMode, label: t("toolbox.tools.formatter.indentTab") },
]);

const stats = computed(() => {
  if (!output.value) {
    return "";
  }
  return t("toolbox.tools.formatter.stats", {
    chars: output.value.length,
    lines: output.value.split("\n").length,
  });
});

/** 依据内容特征猜测语言；猜不准时用户可手动切换。 */
function detectLang(source: string): Exclude<FormatLang, "auto"> {
  const text = source.trim();
  if (!text) {
    return "json";
  }
  if (text.startsWith("{") || text.startsWith("[")) {
    try {
      JSON.parse(text);
      return "json";
    } catch {
      // 非法 JSON，可能是 JS 对象字面量，继续后续判断
    }
  }
  if (/^<template[\s>]/i.test(text) || /^<script\s+setup/i.test(text)) {
    return "vue";
  }
  if (/^<(!doctype|html|head|body|\?xml|svg|div|section)/i.test(text)) {
    return "html";
  }
  if (/^---\s*$/m.test(text) || (!/[{}();=]/.test(text) && /^[\w$-]+\s*:\s*\S/im.test(text))) {
    return "yaml";
  }
  if (
    /\b(interface|enum|implements|namespace|declare)\s+\w+/.test(text) ||
    /\btype\s+\w+\s*=/.test(text) ||
    /:\s*(string|number|boolean|void|any|unknown)\b/.test(text) ||
    /\bas\s+(const|string|number)\b/.test(text)
  ) {
    return "typescript";
  }
  if (
    /\{/.test(text) &&
    /[\w-]+\s*:\s*[^:;{}]+;/.test(text) &&
    !/=>|\b(function|return|const|let|var|new|import|export)\b/.test(text)
  ) {
    return "css";
  }
  return "babel";
}

async function format() {
  const source = input.value;
  if (!source.trim()) {
    ElMessage.warning(t("toolbox.tools.formatter.emptyWarn"));
    return;
  }
  formatting.value = true;
  errorMsg.value = "";
  try {
    const lang = language.value === "auto" ? detectLang(source) : language.value;
    detectedLang.value = lang;
    output.value = await prettier.format(source, {
      parser: lang,
      plugins: PRETTIER_PLUGINS,
      tabWidth: indent.value === "tab" ? 2 : Number(indent.value),
      useTabs: indent.value === "tab",
    });
  } catch (e) {
    output.value = "";
    detectedLang.value = "";
    errorMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    formatting.value = false;
  }
}

function clearAll() {
  input.value = "";
  output.value = "";
  detectedLang.value = "";
  errorMsg.value = "";
}

async function copyResult() {
  await copyToClipboard(output.value);
  ElMessage.success(t("toolbox.tools.formatter.copied"));
}
</script>

<template>
  <div class="view-root">
    <div class="flex gap-1.5 mb-2.5">
      <el-select v-model="language" size="small" class="flex-1">
        <el-option
          v-for="o in langOptions"
          :key="o.value"
          :value="o.value"
          :label="o.label"
        />
      </el-select>
      <el-select v-model="indent" size="small" class="w-[88px]">
        <el-option
          v-for="o in indentOptions"
          :key="o.value"
          :value="o.value"
          :label="o.label"
        />
      </el-select>
    </div>

    <el-scrollbar class="view-scroll">
      <el-input
        v-model="input"
        type="textarea"
        :rows="6"
        :placeholder="t('toolbox.tools.formatter.inputPh')"
      />
      <div class="flex gap-2 mt-2.5">
        <el-button
          type="primary"
          size="small"
          :loading="formatting"
          :icon="MagicStick"
          @click="format"
        >
          {{ t("toolbox.tools.formatter.format") }}
        </el-button>
        <el-button size="small" :icon="Delete" :disabled="!input && !output" @click="clearAll">
          {{ t("toolbox.tools.formatter.clear") }}
        </el-button>
      </div>

      <div
        v-if="language === 'auto' && detectedLang"
        class="text-[10.5px] text-dp-muted mt-1.5"
      >
        {{ t("toolbox.tools.formatter.detected", { lang: LANG_LABELS[detectedLang] }) }}
      </div>

      <el-alert v-if="errorMsg" type="error" :closable="false" class="mt-2.5">
        <pre class="whitespace-pre-wrap text-[10.5px] leading-[1.5] m-0">{{ errorMsg }}</pre>
      </el-alert>

      <el-input
        v-model="output"
        type="textarea"
        :rows="8"
        readonly
        :placeholder="t('toolbox.tools.formatter.outputPh')"
        class="mt-2.5"
      />
      <div v-if="output" class="flex justify-end items-center gap-2 mt-1.5">
        <span class="text-[10px] text-dp-muted">{{ stats }}</span>
        <el-button size="small" text type="primary" :icon="DocumentCopy" @click="copyResult">
          {{ t("toolbox.tools.formatter.copyResult") }}
        </el-button>
      </div>
    </el-scrollbar>
  </div>
</template>
