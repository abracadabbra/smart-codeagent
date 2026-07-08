/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // 使用 CSS 变量，支持 body.light 切换亮色主题
        ink: {
          50: "var(--ink-100)",
          100: "var(--ink-100)",
          200: "var(--ink-200)",
          300: "var(--ink-300)",
          400: "var(--ink-400)",
          500: "var(--ink-500)",
          600: "var(--ink-600)",
          700: "var(--ink-700)",
          800: "var(--ink-800)",
          850: "var(--ink-850)",
          900: "var(--ink-900)",
          950: "var(--ink-950)",
        },
        surface: {
          DEFAULT: "var(--ink-900)",
          raised: "var(--ink-850)",
          overlay: "var(--ink-800)",
          hover: "var(--ink-700)",
        },
        brand: {
          400: "#60a5fa",
          500: "#3b82f6",
          600: "#2563eb",
        },
        accent: {
          DEFAULT: "#22c55e",
          hover: "#16a34a",
        },
      },
      boxShadow: {
        "input-card":
          "0 0 0 1px rgba(39, 39, 42, 0.8), 0 8px 40px rgba(0, 0, 0, 0.5)",
        "input-card-focus":
          "0 0 0 1px rgba(59, 130, 246, 0.5), 0 8px 40px rgba(0, 0, 0, 0.55)",
      },
      keyframes: {
        blink: {
          "0%, 50%": { opacity: "1" },
          "50.01%, 100%": { opacity: "0" },
        },
        "fade-in": {
          "0%": { opacity: "0", transform: "translateY(6px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
      },
      animation: {
        "cursor-blink": "blink 1s steps(2) infinite",
        "fade-in": "fade-in 0.3s ease-out",
      },
    },
  },
  plugins: [],
};
