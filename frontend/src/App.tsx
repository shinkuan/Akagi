import { useEffect } from 'react'
import { Outlet, useLocation } from 'react-router-dom'
import { Sidebar } from '@/components/sidebar/Sidebar'
import { Statusbar } from '@/components/Statusbar'
import { ErrorBoundary } from '@/components/ErrorBoundary'
import { Toaster } from '@/components/ui/sonner'
import { UpdateNotifier } from '@/components/UpdateNotifier'
import { useTauriBridge } from '@/hooks/useTauriBridge'
import { useSidebar } from '@/hooks/useSidebar'
import { useUiPrefsStore } from '@/stores/uiPrefsStore'
import { cn } from '@/lib/utils'

export default function App() {
  useTauriBridge()
  const location = useLocation()
  const scale = useUiPrefsStore((s) => s.scale)
  const isOpen = useSidebar((s) => s.isOpen)
  const settings = useSidebar((s) => s.settings)

  // Apply UI scale by adjusting :root font-size — the design system is
  // rem-based so this scales typography, spacing, icons, sidebar width
  // (w-[18rem] / w-[5.625rem]), and Radix portals uniformly.
  useEffect(() => {
    document.documentElement.style.fontSize = `${16 * scale}px`
    return () => {
      document.documentElement.style.fontSize = ''
    }
  }, [scale])

  return (
    <>
      <Sidebar />
      <main
        className={cn(
          'h-screen flex flex-col min-w-0 overflow-hidden bg-background text-foreground transition-[margin-left] ease-in-out duration-300',
          // Margin tracks the *pinned* width only — hover-open expands the
          // sidebar over main content (z-20) without shifting layout, so
          // width-sensitive widgets like the GameDashboard's react-grid-layout
          // don't re-flow every time the cursor brushes the sidebar.
          settings.disabled
            ? 'lg:ml-0'
            : isOpen
              ? 'lg:ml-[18rem]'
              : 'lg:ml-[5.625rem]',
        )}
      >
        <div className="flex-1 overflow-auto">
          {/* Keyed by route so a crash on one page doesn't poison the others. */}
          <ErrorBoundary key={location.pathname}>
            <Outlet />
          </ErrorBoundary>
        </div>
        <Statusbar />
      </main>
      <Toaster />
      <UpdateNotifier />
    </>
  )
}
