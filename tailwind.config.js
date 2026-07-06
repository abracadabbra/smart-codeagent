/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Deep dark UI tokens matching the Qoder/Quest screenshot
        ink: {
          50: "#fafafa",
          100: "#f4f4f5",
          200: "#e4e4e7",
          300: "#a1a1aa",
          400: "#71717a",
          500: "#52525b",
          600: "#3f3f46",
          700: "#27272a",
          800: "#18181b",
          850: "#121216",
          900: "#0c0c0f",
          950: "#08080a",
        },
        surface: {
          DEFAULT: "#0c0c0f",
          raised: "#121216",
          overlay: "#18181b",
          hover: "#1f1f23",
        },
        brand: {
          400: "#60a5fa",
          500: "#3b82f6",
          600: "#2563eb",
        },
        accent: {
          DEFAULT: "#22c55e", // screenshot green send button
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
