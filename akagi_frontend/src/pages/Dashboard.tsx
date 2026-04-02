import { use, useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ToastContainer } from 'react-toastify';

import { LaunchScreen } from '@/components/LaunchScreen';
import { Footer } from '@/components/layout/Footer';
import { Header } from '@/components/layout/Header';
import SettingsPanel from '@/components/SettingsPanel';
import StreamPlayer from '@/components/StreamPlayer';
import { ConfirmationDialog } from '@/components/ui/confirmation-dialog';
import { APP_SPLASH_DELAY_MS, TOAST_DURATION_DEFAULT } from '@/config/constants';
import { PLATFORM_DEFAULTS, PLATFORMS } from '@/config/platforms';
import { GameContext } from '@/contexts/GameContext';
import { fetchSettingsApi, useSettings } from '@/hooks/useSettings';
import { useTheme } from '@/hooks/useTheme';
import { notify } from '@/lib/notify';
import { cn } from '@/lib/utils';
import type { ResourceStatus, Settings } from '@/types';

interface DashboardProps {
  settingsPromise: Promise<Settings>;
}

function Dashboard({ settingsPromise }: DashboardProps) {
  const { t, i18n } = useTranslation();
  const { theme } = useTheme();
  const initialSettings = use(settingsPromise);

  const context = use(GameContext);
  if (!context) throw new Error('GameContext not found');

  const { updateSetting } = useSettings();

  const handleLocaleChange = useCallback(
    async (newLocale: string) => {
      updateSetting(['locale'], newLocale as string);
    },
    [updateSetting],
  );

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [showShutdownConfirm, setShowShutdownConfirm] = useState(false);
  const [isLaunching, setIsLaunching] = useState(false);
  const [showSplash, setShowSplash] = useState(true);
  const [isMounted, setIsMounted] = useState(false);
  const [resourceStatus, setResourceStatus] = useState<{
    lib: boolean;
    models: boolean;
  } | null>(null);

  useEffect(() => {
    setIsMounted(true);
    const timer = setTimeout(() => {
      setShowSplash(false);
    }, APP_SPLASH_DELAY_MS);

    // 检查可选/关键资源
    window.electron.invoke('check-resource-status').then((status) => {
      setResourceStatus(status as ResourceStatus);
    });

    // 监听来自 Electron 的 HUD 可见性变化（例如窗口关闭/隐藏）
    const unsubHud = window.electron.on('hud-visibility-changed', (visible) => {
      context.setIsHudActive(visible as boolean);
    });

    return () => {
      clearTimeout(timer);
      if (unsubHud) unsubHud();
    };
  }, [context]);

  // 资源状态通知
  useEffect(() => {
    if (!resourceStatus) return;

    if (!resourceStatus.lib) {
      notify.error(t('status_messages.lib_missing'), { toastId: 'lib_missing', autoClose: false });
    }
    if (!resourceStatus.models && !initialSettings.ot.online) {
      notify.warn(t('status_messages.models_missing'), {
        toastId: 'models_missing',
        autoClose: false,
      });
    }
  }, [resourceStatus, t, initialSettings.ot.online]);

  const handleLaunchGame = useCallback(async () => {
    setIsLaunching(true);
    try {
      // 启动前重新拉取设置，确保配置最新
      const currentSettings = await fetchSettingsApi().catch(() => initialSettings);
      const fallbackUrl =
        PLATFORM_DEFAULTS[currentSettings.platform]?.url ?? PLATFORM_DEFAULTS[PLATFORMS.MAJSOUL].url;
      const launchUrl = currentSettings.game_url?.trim() || fallbackUrl;

      // 将 URL、MITM 状态与平台配置传给 Electron
      await window.electron.invoke('start-game', {
        url: launchUrl,
        useMitm: currentSettings.mitm.enabled,
        platform: currentSettings.platform,
      });
    } catch (e) {
      console.error('Failed to start game window:', e);
      notify.error(t('app.launch_error'));
    } finally {
      setIsLaunching(false);
    }
  }, [initialSettings, t]);

  const handleShutdownClick = useCallback(() => {
    setShowShutdownConfirm(true);
  }, []);

  const performShutdown = useCallback(async () => {
    try {
      await window.electron.invoke('request-shutdown');
    } catch (e) {
      console.error('Failed to shutdown:', e);
      notify.error(`${t('common.error')}: ${(e as Error).message}`);
    }
  }, [t]);

  const handleOpenSettings = useCallback(() => setSettingsOpen(true), []);
  const handleCloseSettings = useCallback(() => setSettingsOpen(false), []);
  const handleToggleHud = useCallback(
    (show: boolean) => {
      window.electron.invoke('toggle-hud', show);
      context.setIsHudActive(show);
    },
    [context],
  );

  return (
    <div className='relative flex h-screen flex-col overflow-hidden text-zinc-900 dark:text-zinc-50'>
      {showSplash && (
        <LaunchScreen
          isStatic
          className='animate-out fade-out zoom-out-95 fill-mode-forwards pointer-events-none fixed inset-0 z-50 duration-1000'
        />
      )}

      <div
        className={cn(
          'ease-premium flex h-full flex-col transition-all duration-1000',
          isMounted ? 'blur-0 opacity-100' : 'opacity-0 blur-xl',
        )}
      >
        <Header
          isLaunching={isLaunching}
          onLaunch={handleLaunchGame}
          onOpenSettings={handleOpenSettings}
          locale={i18n.language}
          onLocaleChange={handleLocaleChange}
          onShutdown={handleShutdownClick}
          onToggleHud={handleToggleHud}
          isHudActive={context.isHudActive}
        />
        <main className='mx-auto flex w-full grow flex-col items-center justify-center overflow-hidden px-4 py-4 sm:px-6'>
          <div className='flex h-full w-full flex-col items-center justify-center'>
            <StreamPlayer className='h-full w-full' />
          </div>
        </main>

        <Footer />
      </div>

      <SettingsPanel open={settingsOpen} onClose={handleCloseSettings} />

      <ConfirmationDialog
        open={showShutdownConfirm}
        onOpenChange={setShowShutdownConfirm}
        title={t('app.shutdown_confirm_title')}
        description={t('app.shutdown_confirm_desc')}
        onConfirm={performShutdown}
        variant='destructive'
        confirmText={t('common.confirm')}
        cancelText={t('common.cancel')}
      />
      <ToastContainer
        autoClose={TOAST_DURATION_DEFAULT}
        position='top-right'
        theme={
          theme === 'system'
            ? window.matchMedia('(prefers-color-scheme: dark)').matches
              ? 'dark'
              : 'light'
            : theme
        }
      />
    </div>
  );
}

export default Dashboard;
