import 'react-toastify/dist/ReactToastify.css';

import { lazy, Suspense, use, useEffect, useMemo, useState } from 'react';
import { HashRouter, Route, Routes } from 'react-router-dom';

import { ConnectionProvider } from '@/components/ConnectionProvider';
import { ExitOverlay } from '@/components/ExitOverlay';
import { GameProvider } from '@/components/GameProvider';
import { LaunchScreen } from '@/components/LaunchScreen';
import { SettingsProvider } from '@/components/SettingsProvider';
import { ThemeProvider } from '@/components/ThemeProvider';
import { APP_STARTUP_MIN_DELAY_MS } from '@/config/constants';
import { PLATFORM_DEFAULTS, PLATFORMS } from '@/config/platforms';
import { useConnectionConfig } from '@/hooks/useConnectionConfig';
import { fetchSettingsApi } from '@/hooks/useSettings';
import { useTilePreloader } from '@/hooks/useTilePreloader';
import { setBaseUrl } from '@/lib/api-client';
import type { Settings } from '@/types';

const Dashboard = lazy(() => import('@/pages/Dashboard'));
const Hud = lazy(() => import('@/pages/HUD'));

function AppContent({ settingsPromise }: { settingsPromise: Promise<Settings> }) {
  const initialSettings = use(settingsPromise);
  return (
    <SettingsProvider initialSettings={initialSettings}>
      <GameProvider>
        <HashRouter>
          <Routes>
            <Route path='/' element={<Dashboard settingsPromise={settingsPromise} />} />
            <Route path='/hud' element={<Hud />} />
          </Routes>
        </HashRouter>
      </GameProvider>
    </SettingsProvider>
  );
}

function AppInner() {
  useTilePreloader();
  const { apiBase } = useConnectionConfig();

  const isHud = useMemo(() => window.location.hash === '#/hud', []);

  useEffect(() => {
    document.documentElement.classList.toggle('is-hud', isHud);
    return () => {
      document.documentElement.classList.remove('is-hud');
    };
  }, [isHud]);

  useEffect(() => {
    setBaseUrl(apiBase);
  }, [apiBase]);

  const settingsPromise = useMemo(() => {
    const fetchSettings = (async () => {
      await window.electron.invoke('wait-for-backend');
      setBaseUrl(apiBase);
      return fetchSettingsApi().catch((err) => {
        console.warn('Failed to fetch settings, using defaults:', err);
        return {
          log_level: 'INFO',
          locale: 'zh-CN',
          game_url: PLATFORM_DEFAULTS[PLATFORMS.MAJSOUL].url,
          platform: PLATFORMS.MAJSOUL,
          mitm: { enabled: false, host: '127.0.0.1', port: 6789, upstream: '' },
          server: {
            host: apiBase.split('://')[1]?.split(':')[0] || '127.0.0.1',
            port: parseInt(apiBase.split(':')[2]) || 8765,
          },
          ot: { online: false, server: '', api_key: '' },
          model_config: {
            model_4p: '',
            model_3p: '',
            temperature: 0.3,
            rule_based_agari_guard: true,
          },
        } satisfies Settings;
      });
    })();

    if (isHud) {
      return fetchSettings;
    }

    const minDelay = new Promise<void>((resolve) => setTimeout(resolve, APP_STARTUP_MIN_DELAY_MS));
    return Promise.all([fetchSettings, minDelay]).then(([settings]) => settings);
  }, [apiBase, isHud]);

  const [isExiting, setIsExiting] = useState(false);

  useEffect(() => {
    const unsubExit = window.electron.on('exit-animation-start', () => {
      setIsExiting(true);
    });

    return () => unsubExit();
  }, []);

  return (
    <Suspense
      fallback={isHud ? <div className='h-screen w-screen bg-transparent' /> : <LaunchScreen />}
    >
      <ThemeProvider>
        <AppContent settingsPromise={settingsPromise} />
        {isExiting && <ExitOverlay />}
      </ThemeProvider>
    </Suspense>
  );
}

export default function App() {
  return (
    <ConnectionProvider>
      <AppInner />
    </ConnectionProvider>
  );
}
