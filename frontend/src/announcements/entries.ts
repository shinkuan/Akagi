import type { LucideIcon } from 'lucide-react'
import {
  AppWindow,
  Bot,
  Brain,
  CloudCog,
  CreditCard,
  Download,
  Gamepad2,
  RefreshCw,
  SearchCheck,
  SlidersHorizontal,
  Zap,
} from 'lucide-react'

import { AKAGIMS_DOWNLOAD_URL } from '@/lib/external'
import akagimsScreenshot from '@/assets/akagims-fullauto.jpg'
import mjotlogodarkbg from '@/assets/mjot-logo-dark-bg.png'

/** One feature highlight inside an announcement's expanded view. */
export type AnnouncementFeature = {
  icon: LucideIcon
  /**
   * i18n leaf name: the dialog reads
   * `announcements.entries.<id>.<key>_title` and `…_desc`.
   */
  key: string
}

export type AnnouncementEntry = {
  /** Stable i18n slug: strings live under `announcements.entries.<id>`. */
  id: string
  /**
   * Publish date, ISO `YYYY-MM-DD`. Must be unique and strictly
   * descending down the array — it doubles as the "which announcements
   * has the user seen" ordering.
   */
  date: string
  /**
   * For release announcements: the exact Cargo.toml package version,
   * rendered as a badge on the row. Product news entries omit this.
   * Purely cosmetic — it does not gate visibility. Entries are bundled
   * into the build, so a client that can see an entry is already on the
   * matching release (or newer); there is nothing to hide.
   */
  version?: string
  /** Optional bundled image shown expanded; requires `<id>.image_alt`. */
  image?: string
  /** Optional external action URL; requires `<id>.link_label`. */
  link?: string
  features: AnnouncementFeature[]
}

/**
 * All in-app announcements, newest first. Add an entry (plus locale
 * strings in all four i18n resources) for every release BEFORE tagging
 * it — the release tagging script refuses to tag a version that has no
 * committed entry here. See README.md in this directory for the workflow.
 */
export const ANNOUNCEMENTS: AnnouncementEntry[] = [
  {
    id: 'v3_8_0',
    date: '2026-11-04',
    version: '3.8.0',
    features: [
      { icon: RefreshCw, key: 'tenhou_rejoin' },
      { icon: Brain, key: 'think_time' },
      { icon: SlidersHorizontal, key: 'delay_settings' },
    ],
  },
  {
    id: 'v3_7_0',
    date: '2026-08-25',
    version: '3.7.0',
    features: [
      { icon: Bot, key: 'rc_autoplay' },
      { icon: SearchCheck, key: 'review' },
    ],
  },
  {
    id: 'v3_6_0',
    date: '2026-08-14',
    version: '3.6.0',
    features: [
      { icon: Bot, key: 'tenhou_autoplay' },
      { icon: Download, key: 'update_source' },
    ],
  },
  {
    id: 'v3_5_0',
    date: '2026-08-12',
    version: '3.5.0',
    image: mjotlogodarkbg,
    features: [
      { icon: CreditCard, key: 'checkout' },
      { icon: CloudCog, key: 'health' },
    ],
  },
  {
    id: 'akagims',
    date: '2026-08-09',
    image: akagimsScreenshot,
    link: AKAGIMS_DOWNLOAD_URL,
    features: [
      { icon: Gamepad2, key: 'majsoul' },
      { icon: AppWindow, key: 'embedded' },
      { icon: Bot, key: 'fullauto' },
      { icon: Zap, key: 'zero_setup' },
    ],
  },
]
