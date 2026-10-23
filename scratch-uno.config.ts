import { defineConfig, presetWind3 } from "unocss";
export default defineConfig({
  presets: [presetWind3()],
  shortcuts: {
    "md-body": "min-w-0",
    "md-foo": "flex",
    "ax-body": "grid",
  },
});
