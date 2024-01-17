# -*- coding: utf-8 -*-
# Generated.  DO NOT EDIT!

import ms_tournament.protocol_admin_pb2 as pb
from ms_tournament.base import MSRPCService


class CustomizedContestManagerApi(MSRPCService):
    version = None
    
    _req = {
        'loginContestManager': pb.ReqContestManageLogin,
        'oauth2AuthContestManager': pb.ReqContestManageOauth2Auth,
        'oauth2LoginContestManager': pb.ReqContestManageOauth2Login,
        'logoutContestManager': pb.ReqCommon,
        'fetchRelatedContestList': pb.ReqCommon,
        'createContest': pb.ReqCreateCustomizedContest,
        'deleteContest': pb.ReqDeleteCustomizedContest,
        'prolongContest': pb.ReqProlongContest,
        'manageContest': pb.ReqManageContest,
        'fetchContestInfo': pb.ReqCommon,
        'exitManageContest': pb.ReqCommon,
        'fetchContestGameRule': pb.ReqCommon,
        'updateContestGameRule': pb.ReqUpdateContestGameRule,
        'searchAccountByNickname': pb.ReqSearchAccountByNickname,
        'searchAccountByEid': pb.ReqSearchAccountByEid,
        'fetchContestPlayer': pb.ReqCommon,
        'updateContestPlayer': pb.ReqUpdateCustomizedContestPlayer,
        'startManageGame': pb.ReqCommon,
        'stopManageGame': pb.ReqCommon,
        'lockGamePlayer': pb.ReqLockGamePlayer,
        'unlockGamePlayer': pb.ReqUnlockGamePlayer,
        'createContestGame': pb.ReqCreateContestGame,
        'fetchContestGameRecords': pb.ReqFetchCustomizedContestGameRecordList,
        'removeContestGameRecord': pb.ReqRemoveContestGameRecord,
        'fetchContestNotice': pb.ReqFetchContestNotice,
        'updateContestNotice': pb.ReqUpdateCustomizedContestNotice,
        'fetchContestManager': pb.ReqCommon,
        'updateContestManager': pb.ReqUpdateCustomizedContestManager,
        'fetchChatSetting': pb.ReqCommon,
        'updateChatSetting': pb.ReqUpdateCustomizedContestChatSetting,
        'updateGameTag': pb.ReqUpdateGameTag,
        'terminateGame': pb.ReqTerminateContestGame,
        'pauseGame': pb.ReqPauseContestGame,
        'resumeGame': pb.ReqResumeContestGame,
        'fetchCurrentRankList': pb.ReqCommon,
        'fetchContestLastModify': pb.ReqCommon,
        'fetchContestObserver': pb.ReqCommon,
        'addContestObserver': pb.ReqAddContestObserver,
        'removeContestObserver': pb.ReqRemoveContestObserver,
        'fetchContestChatHistory': pb.ReqCommon,
        'clearChatHistory': pb.ReqClearChatHistory,
    }
    _res = {
        'loginContestManager': pb.ResContestManageLogin,
        'oauth2AuthContestManager': pb.ResContestManageOauth2Auth,
        'oauth2LoginContestManager': pb.ResContestManageOauth2Login,
        'logoutContestManager': pb.ResCommon,
        'fetchRelatedContestList': pb.ResFetchRelatedContestList,
        'createContest': pb.ResCreateCustomizedContest,
        'deleteContest': pb.ResCommon,
        'prolongContest': pb.ResProlongContest,
        'manageContest': pb.ResManageContest,
        'fetchContestInfo': pb.ResManageContest,
        'exitManageContest': pb.ResCommon,
        'fetchContestGameRule': pb.ResFetchContestGameRule,
        'updateContestGameRule': pb.ResCommon,
        'searchAccountByNickname': pb.ResSearchAccountByNickname,
        'searchAccountByEid': pb.ResSearchAccountByEid,
        'fetchContestPlayer': pb.ResFetchCustomizedContestPlayer,
        'updateContestPlayer': pb.ResCommon,
        'startManageGame': pb.ResStartManageGame,
        'stopManageGame': pb.ResCommon,
        'lockGamePlayer': pb.ResCommon,
        'unlockGamePlayer': pb.ResCommon,
        'createContestGame': pb.ResCreateContestGame,
        'fetchContestGameRecords': pb.ResFetchCustomizedContestGameRecordList,
        'removeContestGameRecord': pb.ResCommon,
        'fetchContestNotice': pb.ResFetchContestNotice,
        'updateContestNotice': pb.ResCommon,
        'fetchContestManager': pb.ResFetchCustomizedContestManager,
        'updateContestManager': pb.ResCommon,
        'fetchChatSetting': pb.ResCustomizedContestChatInfo,
        'updateChatSetting': pb.ResUpdateCustomizedContestChatSetting,
        'updateGameTag': pb.ResCommon,
        'terminateGame': pb.ResCommon,
        'pauseGame': pb.ResCommon,
        'resumeGame': pb.ResCommon,
        'fetchCurrentRankList': pb.ResFetchCurrentRankList,
        'fetchContestLastModify': pb.ResFetchContestLastModify,
        'fetchContestObserver': pb.ResFetchContestObserver,
        'addContestObserver': pb.ResAddContestObserver,
        'removeContestObserver': pb.ResCommon,
        'fetchContestChatHistory': pb.ResFetchContestChatHistory,
        'clearChatHistory': pb.ResCommon,
    }

    def get_package_name(self):
        return 'lq'

    def get_service_name(self):
        return 'CustomizedContestManagerApi'

    def get_req_class(self, method):
        return CustomizedContestManagerApi._req[method]

    def get_res_class(self, method):
        return CustomizedContestManagerApi._res[method]

    async def login_contest_manager(self, req):
        return await self.call_method('loginContestManager', req)

    async def oauth2_auth_contest_manager(self, req):
        return await self.call_method('oauth2AuthContestManager', req)

    async def oauth2_login_contest_manager(self, req):
        return await self.call_method('oauth2LoginContestManager', req)

    async def logout_contest_manager(self, req):
        return await self.call_method('logoutContestManager', req)

    async def fetch_related_contest_list(self, req):
        return await self.call_method('fetchRelatedContestList', req)

    async def create_contest(self, req):
        return await self.call_method('createContest', req)

    async def delete_contest(self, req):
        return await self.call_method('deleteContest', req)

    async def prolong_contest(self, req):
        return await self.call_method('prolongContest', req)

    async def manage_contest(self, req):
        return await self.call_method('manageContest', req)

    async def fetch_contest_info(self, req):
        return await self.call_method('fetchContestInfo', req)

    async def exit_manage_contest(self, req):
        return await self.call_method('exitManageContest', req)

    async def fetch_contest_game_rule(self, req):
        return await self.call_method('fetchContestGameRule', req)

    async def update_contest_game_rule(self, req):
        return await self.call_method('updateContestGameRule', req)

    async def search_account_by_nickname(self, req):
        return await self.call_method('searchAccountByNickname', req)

    async def search_account_by_eid(self, req):
        return await self.call_method('searchAccountByEid', req)

    async def fetch_contest_player(self, req):
        return await self.call_method('fetchContestPlayer', req)

    async def update_contest_player(self, req):
        return await self.call_method('updateContestPlayer', req)

    async def start_manage_game(self, req):
        return await self.call_method('startManageGame', req)

    async def stop_manage_game(self, req):
        return await self.call_method('stopManageGame', req)

    async def lock_game_player(self, req):
        return await self.call_method('lockGamePlayer', req)

    async def unlock_game_player(self, req):
        return await self.call_method('unlockGamePlayer', req)

    async def create_contest_game(self, req):
        return await self.call_method('createContestGame', req)

    async def fetch_contest_game_records(self, req):
        return await self.call_method('fetchContestGameRecords', req)

    async def remove_contest_game_record(self, req):
        return await self.call_method('removeContestGameRecord', req)

    async def fetch_contest_notice(self, req):
        return await self.call_method('fetchContestNotice', req)

    async def update_contest_notice(self, req):
        return await self.call_method('updateContestNotice', req)

    async def fetch_contest_manager(self, req):
        return await self.call_method('fetchContestManager', req)

    async def update_contest_manager(self, req):
        return await self.call_method('updateContestManager', req)

    async def fetch_chat_setting(self, req):
        return await self.call_method('fetchChatSetting', req)

    async def update_chat_setting(self, req):
        return await self.call_method('updateChatSetting', req)

    async def update_game_tag(self, req):
        return await self.call_method('updateGameTag', req)

    async def terminate_game(self, req):
        return await self.call_method('terminateGame', req)

    async def pause_game(self, req):
        return await self.call_method('pauseGame', req)

    async def resume_game(self, req):
        return await self.call_method('resumeGame', req)

    async def fetch_current_rank_list(self, req):
        return await self.call_method('fetchCurrentRankList', req)

    async def fetch_contest_last_modify(self, req):
        return await self.call_method('fetchContestLastModify', req)

    async def fetch_contest_observer(self, req):
        return await self.call_method('fetchContestObserver', req)

    async def add_contest_observer(self, req):
        return await self.call_method('addContestObserver', req)

    async def remove_contest_observer(self, req):
        return await self.call_method('removeContestObserver', req)

    async def fetch_contest_chat_history(self, req):
        return await self.call_method('fetchContestChatHistory', req)

    async def clear_chat_history(self, req):
        return await self.call_method('clearChatHistory', req)
