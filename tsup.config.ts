import { defineConfig } from "tsup";

export default defineConfig({
  name: "flakehub-push",
  entry: ["ts/index.ts"],
  format: ["esm"],
  target: "node20",
  bundle: true,
  splitting: false,
  sourcemap: true,
  clean: true,
  dts: {
    resolve: true,
  },
});
