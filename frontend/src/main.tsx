import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider } from 'react-router-dom'
import 'mahgen'
import './index.css'
import './i18n'
import './stores/themeStore'
import { Overlay } from '@/routes/Overlay'
import { MainWindowRoot } from '@/components/MainWindowRoot'
import { router } from '@/router'
import { TopToolbar } from '@/components/TopToolbar'
import { isMainWebview, isOverlayWindow, isTopWindow } from '@/lib/window'

// Both windows load this same bundle; the window label decides which root gets
// mounted. The overlay skips the router entirely — it has exactly one screen,
// and a hash route would only invite the sidebar/statusbar layout to tag along.
//
// The single-window layout adds two more labels: `top` (the toolbar strip) and
// the `main` webview, which becomes the right-hand panel once the wizard has
// run. Detached pages (`tool-*`) get the full router, so the rich pages are
// unchanged.
const root = createRoot(document.getElementById('root')!)

if (isOverlayWindow()) {
  // Lets index.css punch the page background out to transparent, so only the
  // overlay's own card is drawn over the game.
  document.documentElement.classList.add('overlay-window')
  root.render(
    <StrictMode>
      <Overlay />
    </StrictMode>,
  )
} else if (isTopWindow()) {
  // The strip is a fixed-height chrome surface: no page scroll, no margins.
  document.documentElement.classList.add('toolbar-window')
  root.render(
    <StrictMode>
      <TopToolbar />
    </StrictMode>,
  )
} else if (isMainWebview()) {
  root.render(
    <StrictMode>
      <MainWindowRoot />
    </StrictMode>,
  )
} else {
  root.render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>,
  )
}
