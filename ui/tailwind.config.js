/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        obsidian: {
          bg: '#1e1e1e',
          sidebar: '#181818',
          card: '#252526',
          border: '#333333',
          accent: '#7c3aed',
          text: '#dcddde',
          muted: '#8e8e8e',
        }
      }
    },
  },
  plugins: [],
}
