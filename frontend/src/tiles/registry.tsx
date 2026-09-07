import { HeaderTile } from './HeaderTile'
import { PlayerTile } from './PlayerTile'
import { SelfHandTile } from './SelfHandTile'
import { BoardTile } from './BoardTile'
import { RecommendationsTile } from './RecommendationsTile'
import { ExpectedDrawsTile } from './ExpectedDrawsTile'
import { RiskChartTile } from './RiskChartTile'
import { OpponentsTile } from './OpponentsTile'
import { EventsTile } from './EventsTile'
import { NotificationsTile } from './NotificationsTile'
import { BotResponsesTile } from './BotResponsesTile'
import { BotActionTile } from './BotActionTile'
import { BotShowTile } from './BotShowTile'
import { ProxyControlTile } from './ProxyControlTile'
import type { Breakpoint, TileId } from './defaults'

export function renderTile(id: TileId, bp: Breakpoint) {
  switch (id) {
    case 'header':          return <HeaderTile bp={bp} />
    case 'player-0':        return <PlayerTile seat={0} bp={bp} />
    case 'player-1':        return <PlayerTile seat={1} bp={bp} />
    case 'player-2':        return <PlayerTile seat={2} bp={bp} />
    case 'player-3':        return <PlayerTile seat={3} bp={bp} />
    case 'self-hand':       return <SelfHandTile bp={bp} />
    case 'board':           return <BoardTile bp={bp} />
    case 'recommendations': return <RecommendationsTile bp={bp} />
    case 'expected-draws':  return <ExpectedDrawsTile bp={bp} />
    case 'risk-chart':      return <RiskChartTile bp={bp} />
    case 'opponents':       return <OpponentsTile bp={bp} />
    case 'events':          return <EventsTile bp={bp} />
    case 'notifications':   return <NotificationsTile bp={bp} />
    case 'bot-responses':   return <BotResponsesTile bp={bp} />
    case 'bot-action':      return <BotActionTile bp={bp} />
    case 'bot-show':        return <BotShowTile bp={bp} />
    case 'proxy-control':   return <ProxyControlTile bp={bp} />
  }
}
