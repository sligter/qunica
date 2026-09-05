// Runs before paint without requiring an inline-script CSP exception.
(() => {
  let appearance = null
  try {
    const mirrored = localStorage.getItem('qunica:appearance')
    if (mirrored === 'light' || mirrored === 'dark') appearance = mirrored
  } catch { /* Restricted storage still gets the system theme. */ }
  if (!appearance) {
    appearance = window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  }
  document.documentElement.dataset.theme = appearance
  document.documentElement.style.colorScheme = appearance
})()
