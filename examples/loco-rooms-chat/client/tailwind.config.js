/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {},
  },
  safelist: [
    "text-ctp-red",
    "text-ctp-green",
    "text-ctp-pink",
    "text-ctp-peach",
    "text-ctp-blue",
    "text-ctp-teal",
    "text-ctp-sky",
  ],
  plugins: [
    require("@catppuccin/tailwindcss")({
      prefix: "ctp",
      defaultFlavour: "mocha",
    }),
  ],
};
