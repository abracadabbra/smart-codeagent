/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Qoder-style dark theme tokens
        ink: {
          50: "#fafafa",
          100: "#e4e4e7",
          200: "#a1a1aa",
          300: "#71717a",
          400: "#52525b",
          500: "#3f3f46",
          600: "#27272a",
          700: "#1f1f23",
          800: "#18181b",
          900: "#0f0f12",
        },
      },
      keyframes: {
        blink: {
          "0%, 50%": { opacity: "1" },
          "50.01%, 100%": { opacity: "0" },
        },
      },
      animation: {
        "cursor-blink": "blink 1s steps(2) infinite",
      },
    },
  },
  plugins: [],
};