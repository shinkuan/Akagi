# -*- coding: utf-8 -*-
# Generated.  DO NOT EDIT!

import ms.protocol_pb2 as pb
from ms.base import MSRPCService


class Lobby(MSRPCService):
    version = None
    
    _req = {
        'fetchConnectionInfo': pb.ReqCommon,
        'fetchQueueInfo': pb.ReqCommon,
        'cancelQueue': pb.ReqCommon,
        'openidCheck': pb.ReqOpenidCheck,
        'signup': pb.ReqSignupAccount,
        'login': pb.ReqLogin,
        'fetchInfo': pb.ReqCommon,
        'loginSuccess': pb.ReqCommon,
        'emailLogin': pb.ReqEmailLogin,
        'oauth2Auth': pb.ReqOauth2Auth,
        'oauth2Check': pb.ReqOauth2Check,
        'oauth2Signup': pb.ReqOauth2Signup,
        'oauth2Login': pb.ReqOauth2Login,
        'dmmPreLogin': pb.ReqDMMPreLogin,
        'createPhoneVerifyCode': pb.ReqCreatePhoneVerifyCode,
        'createEmailVerifyCode': pb.ReqCreateEmailVerifyCode,
        'verfifyCodeForSecure': pb.ReqVerifyCodeForSecure,
        'bindPhoneNumber': pb.ReqBindPhoneNumber,
        'unbindPhoneNumber': pb.ReqUnbindPhoneNumber,
        'fetchPhoneLoginBind': pb.ReqCommon,
        'createPhoneLoginBind': pb.ReqCreatePhoneLoginBind,
        'bindEmail': pb.ReqBindEmail,
        'modifyPassword': pb.ReqModifyPassword,
        'bindAccount': pb.ReqBindAccount,
        'logout': pb.ReqLogout,
        'heatbeat': pb.ReqHeatBeat,
        'loginBeat': pb.ReqLoginBeat,
        'createNickname': pb.ReqCreateNickname,
        'modifyNickname': pb.ReqModifyNickname,
        'modifyBirthday': pb.ReqModifyBirthday,
        'fetchRoom': pb.ReqCommon,
        'createRoom': pb.ReqCreateRoom,
        'joinRoom': pb.ReqJoinRoom,
        'leaveRoom': pb.ReqCommon,
        'readyPlay': pb.ReqRoomReady,
        'dressingStatus': pb.ReqRoomDressing,
        'startRoom': pb.ReqRoomStart,
        'kickPlayer': pb.ReqRoomKick,
        'modifyRoom': pb.ReqModifyRoom,
        'matchGame': pb.ReqJoinMatchQueue,
        'cancelMatch': pb.ReqCancelMatchQueue,
        'fetchAccountInfo': pb.ReqAccountInfo,
        'changeAvatar': pb.ReqChangeAvatar,
        'receiveVersionReward': pb.ReqCommon,
        'fetchAccountStatisticInfo': pb.ReqAccountStatisticInfo,
        'fetchAccountChallengeRankInfo': pb.ReqAccountInfo,
        'fetchAccountCharacterInfo': pb.ReqCommon,
        'shopPurchase': pb.ReqShopPurchase,
        'fetchGameRecord': pb.ReqGameRecord,
        'readGameRecord': pb.ReqGameRecord,
        'fetchGameRecordList': pb.ReqGameRecordList,
        'fetchCollectedGameRecordList': pb.ReqCommon,
        'fetchGameRecordsDetail': pb.ReqGameRecordsDetail,
        'addCollectedGameRecord': pb.ReqAddCollectedGameRecord,
        'removeCollectedGameRecord': pb.ReqRemoveCollectedGameRecord,
        'changeCollectedGameRecordRemarks': pb.ReqChangeCollectedGameRecordRemarks,
        'fetchLevelLeaderboard': pb.ReqLevelLeaderboard,
        'fetchChallengeLeaderboard': pb.ReqChallangeLeaderboard,
        'fetchMutiChallengeLevel': pb.ReqMutiChallengeLevel,
        'fetchMultiAccountBrief': pb.ReqMultiAccountId,
        'fetchFriendList': pb.ReqCommon,
        'fetchFriendApplyList': pb.ReqCommon,
        'applyFriend': pb.ReqApplyFriend,
        'handleFriendApply': pb.ReqHandleFriendApply,
        'removeFriend': pb.ReqRemoveFriend,
        'searchAccountById': pb.ReqSearchAccountById,
        'searchAccountByPattern': pb.ReqSearchAccountByPattern,
        'fetchAccountState': pb.ReqAccountList,
        'fetchBagInfo': pb.ReqCommon,
        'useBagItem': pb.ReqUseBagItem,
        'openManualItem': pb.ReqOpenManualItem,
        'openRandomRewardItem': pb.ReqOpenRandomRewardItem,
        'openAllRewardItem': pb.ReqOpenAllRewardItem,
        'composeShard': pb.ReqComposeShard,
        'fetchAnnouncement': pb.ReqFetchAnnouncement,
        'readAnnouncement': pb.ReqReadAnnouncement,
        'fetchMailInfo': pb.ReqCommon,
        'readMail': pb.ReqReadMail,
        'deleteMail': pb.ReqDeleteMail,
        'takeAttachmentFromMail': pb.ReqTakeAttachment,
        'receiveAchievementReward': pb.ReqReceiveAchievementReward,
        'receiveAchievementGroupReward': pb.ReqReceiveAchievementGroupReward,
        'fetchAchievementRate': pb.ReqCommon,
        'fetchAchievement': pb.ReqCommon,
        'buyShiLian': pb.ReqBuyShiLian,
        'matchShiLian': pb.ReqCommon,
        'goNextShiLian': pb.ReqCommon,
        'updateClientValue': pb.ReqUpdateClientValue,
        'fetchClientValue': pb.ReqCommon,
        'clientMessage': pb.ReqClientMessage,
        'fetchCurrentMatchInfo': pb.ReqCurrentMatchInfo,
        'userComplain': pb.ReqUserComplain,
        'fetchReviveCoinInfo': pb.ReqCommon,
        'gainReviveCoin': pb.ReqCommon,
        'fetchDailyTask': pb.ReqCommon,
        'refreshDailyTask': pb.ReqRefreshDailyTask,
        'useGiftCode': pb.ReqUseGiftCode,
        'useSpecialGiftCode': pb.ReqUseGiftCode,
        'fetchTitleList': pb.ReqCommon,
        'useTitle': pb.ReqUseTitle,
        'sendClientMessage': pb.ReqSendClientMessage,
        'fetchGameLiveInfo': pb.ReqGameLiveInfo,
        'fetchGameLiveLeftSegment': pb.ReqGameLiveLeftSegment,
        'fetchGameLiveList': pb.ReqGameLiveList,
        'fetchCommentSetting': pb.ReqCommon,
        'updateCommentSetting': pb.ReqUpdateCommentSetting,
        'fetchCommentList': pb.ReqFetchCommentList,
        'fetchCommentContent': pb.ReqFetchCommentContent,
        'leaveComment': pb.ReqLeaveComment,
        'deleteComment': pb.ReqDeleteComment,
        'updateReadComment': pb.ReqUpdateReadComment,
        'fetchRollingNotice': pb.ReqCommon,
        'fetchServerTime': pb.ReqCommon,
        'fetchPlatformProducts': pb.ReqPlatformBillingProducts,
        'cancelGooglePlayOrder': pb.ReqCancelGooglePlayOrder,
        'openChest': pb.ReqOpenChest,
        'buyFromChestShop': pb.ReqBuyFromChestShop,
        'fetchDailySignInInfo': pb.ReqCommon,
        'doDailySignIn': pb.ReqCommon,
        'doActivitySignIn': pb.ReqDoActivitySignIn,
        'fetchCharacterInfo': pb.ReqCommon,
        'updateCharacterSort': pb.ReqUpdateCharacterSort,
        'changeMainCharacter': pb.ReqChangeMainCharacter,
        'changeCharacterSkin': pb.ReqChangeCharacterSkin,
        'changeCharacterView': pb.ReqChangeCharacterView,
        'setHiddenCharacter': pb.ReqSetHiddenCharacter,
        'sendGiftToCharacter': pb.ReqSendGiftToCharacter,
        'sellItem': pb.ReqSellItem,
        'fetchCommonView': pb.ReqCommon,
        'changeCommonView': pb.ReqChangeCommonView,
        'saveCommonViews': pb.ReqSaveCommonViews,
        'fetchCommonViews': pb.ReqCommonViews,
        'fetchAllCommonViews': pb.ReqCommon,
        'useCommonView': pb.ReqUseCommonView,
        'upgradeCharacter': pb.ReqUpgradeCharacter,
        'addFinishedEnding': pb.ReqFinishedEnding,
        'receiveEndingReward': pb.ReqFinishedEnding,
        'gameMasterCommand': pb.ReqGMCommand,
        'fetchShopInfo': pb.ReqCommon,
        'buyFromShop': pb.ReqBuyFromShop,
        'buyFromZHP': pb.ReqBuyFromZHP,
        'refreshZHPShop': pb.ReqReshZHPShop,
        'fetchMonthTicketInfo': pb.ReqCommon,
        'payMonthTicket': pb.ReqCommon,
        'exchangeCurrency': pb.ReqExchangeCurrency,
        'exchangeChestStone': pb.ReqExchangeCurrency,
        'exchangeDiamond': pb.ReqExchangeCurrency,
        'fetchServerSettings': pb.ReqCommon,
        'fetchAccountSettings': pb.ReqCommon,
        'updateAccountSettings': pb.ReqUpdateAccountSettings,
        'fetchModNicknameTime': pb.ReqCommon,
        'createWechatNativeOrder': pb.ReqCreateWechatNativeOrder,
        'createWechatAppOrder': pb.ReqCreateWechatAppOrder,
        'createAlipayOrder': pb.ReqCreateAlipayOrder,
        'createAlipayScanOrder': pb.ReqCreateAlipayScanOrder,
        'createAlipayAppOrder': pb.ReqCreateAlipayAppOrder,
        'createJPCreditCardOrder': pb.ReqCreateJPCreditCardOrder,
        'createJPPaypalOrder': pb.ReqCreateJPPaypalOrder,
        'createJPAuOrder': pb.ReqCreateJPAuOrder,
        'createJPDocomoOrder': pb.ReqCreateJPDocomoOrder,
        'createJPWebMoneyOrder': pb.ReqCreateJPWebMoneyOrder,
        'createJPSoftbankOrder': pb.ReqCreateJPSoftbankOrder,
        'createJPPayPayOrder': pb.ReqCreateJPPayPayOrder,
        'fetchJPCommonCreditCardOrder': pb.ReqFetchJPCommonCreditCardOrder,
        'createJPGMOOrder': pb.ReqCreateJPGMOOrder,
        'createENPaypalOrder': pb.ReqCreateENPaypalOrder,
        'createENMasterCardOrder': pb.ReqCreateENMasterCardOrder,
        'createENVisaOrder': pb.ReqCreateENVisaOrder,
        'createENJCBOrder': pb.ReqCreateENJCBOrder,
        'createENAlipayOrder': pb.ReqCreateENAlipayOrder,
        'createKRPaypalOrder': pb.ReqCreateKRPaypalOrder,
        'createKRMasterCardOrder': pb.ReqCreateKRMasterCardOrder,
        'createKRVisaOrder': pb.ReqCreateKRVisaOrder,
        'createKRJCBOrder': pb.ReqCreateKRJCBOrder,
        'createKRAlipayOrder': pb.ReqCreateKRAlipayOrder,
        'createDMMOrder': pb.ReqCreateDMMOrder,
        'createIAPOrder': pb.ReqCreateIAPOrder,
        'createSteamOrder': pb.ReqCreateSteamOrder,
        'verifySteamOrder': pb.ReqVerifySteamOrder,
        'createMyCardAndroidOrder': pb.ReqCreateMyCardOrder,
        'createMyCardWebOrder': pb.ReqCreateMyCardOrder,
        'createPaypalOrder': pb.ReqCreatePaypalOrder,
        'createXsollaOrder': pb.ReqCreateXsollaOrder,
        'verifyMyCardOrder': pb.ReqVerifyMyCardOrder,
        'verificationIAPOrder': pb.ReqVerificationIAPOrder,
        'createYostarSDKOrder': pb.ReqCreateYostarOrder,
        'createBillingOrder': pb.ReqCreateBillingOrder,
        'solveGooglePlayOrder': pb.ReqSolveGooglePlayOrder,
        'solveGooglePayOrderV3': pb.ReqSolveGooglePlayOrderV3,
        'deliverAA32Order': pb.ReqDeliverAA32Order,
        'fetchMisc': pb.ReqCommon,
        'modifySignature': pb.ReqModifySignature,
        'fetchIDCardInfo': pb.ReqCommon,
        'updateIDCardInfo': pb.ReqUpdateIDCardInfo,
        'fetchVipReward': pb.ReqCommon,
        'gainVipReward': pb.ReqGainVipReward,
        'fetchRefundOrder': pb.ReqCommon,
        'fetchCustomizedContestList': pb.ReqFetchCustomizedContestList,
        'fetchCustomizedContestExtendInfo': pb.ReqFetchCustomizedContestExtendInfo,
        'fetchCustomizedContestAuthInfo': pb.ReqFetchCustomizedContestAuthInfo,
        'enterCustomizedContest': pb.ReqEnterCustomizedContest,
        'leaveCustomizedContest': pb.ReqCommon,
        'fetchCustomizedContestOnlineInfo': pb.ReqFetchCustomizedContestOnlineInfo,
        'fetchCustomizedContestByContestId': pb.ReqFetchCustomizedContestByContestId,
        'startCustomizedContest': pb.ReqStartCustomizedContest,
        'stopCustomizedContest': pb.ReqCommon,
        'joinCustomizedContestChatRoom': pb.ReqJoinCustomizedContestChatRoom,
        'leaveCustomizedContestChatRoom': pb.ReqCommon,
        'sayChatMessage': pb.ReqSayChatMessage,
        'fetchCustomizedContestGameRecords': pb.ReqFetchCustomizedContestGameRecords,
        'fetchCustomizedContestGameLiveList': pb.ReqFetchCustomizedContestGameLiveList,
        'followCustomizedContest': pb.ReqTargetCustomizedContest,
        'unfollowCustomizedContest': pb.ReqTargetCustomizedContest,
        'fetchActivityList': pb.ReqCommon,
        'fetchAccountActivityData': pb.ReqCommon,
        'exchangeActivityItem': pb.ReqExchangeActivityItem,
        'completeActivityTask': pb.ReqCompleteActivityTask,
        'completeActivityFlipTask': pb.ReqCompleteActivityTask,
        'completePeriodActivityTask': pb.ReqCompleteActivityTask,
        'completePeriodActivityTaskBatch': pb.ReqCompletePeriodActivityTaskBatch,
        'completeRandomActivityTask': pb.ReqCompleteActivityTask,
        'receiveActivityFlipTask': pb.ReqReceiveActivityFlipTask,
        'completeSegmentTaskReward': pb.ReqCompleteSegmentTaskReward,
        'fetchActivityFlipInfo': pb.ReqFetchActivityFlipInfo,
        'gainAccumulatedPointActivityReward': pb.ReqGainAccumulatedPointActivityReward,
        'gainMultiPointActivityReward': pb.ReqGainMultiPointActivityReward,
        'fetchRankPointLeaderboard': pb.ReqFetchRankPointLeaderboard,
        'gainRankPointReward': pb.ReqGainRankPointReward,
        'richmanActivityNextMove': pb.ReqRichmanNextMove,
        'richmanAcitivitySpecialMove': pb.ReqRichmanSpecialMove,
        'richmanActivityChestInfo': pb.ReqRichmanChestInfo,
        'createGameObserveAuth': pb.ReqCreateGameObserveAuth,
        'refreshGameObserveAuth': pb.ReqRefreshGameObserveAuth,
        'fetchActivityBuff': pb.ReqCommon,
        'upgradeActivityBuff': pb.ReqUpgradeActivityBuff,
        'upgradeActivityLevel': pb.ReqUpgradeActivityLevel,
        'receiveUpgradeActivityReward': pb.ReqReceiveUpgradeActivityReward,
        'upgradeChallenge': pb.ReqCommon,
        'refreshChallenge': pb.ReqCommon,
        'fetchChallengeInfo': pb.ReqCommon,
        'forceCompleteChallengeTask': pb.ReqForceCompleteChallengeTask,
        'fetchChallengeSeason': pb.ReqCommon,
        'receiveChallengeRankReward': pb.ReqReceiveChallengeRankReward,
        'fetchABMatchInfo': pb.ReqCommon,
        'buyInABMatch': pb.ReqBuyInABMatch,
        'receiveABMatchReward': pb.ReqCommon,
        'quitABMatch': pb.ReqCommon,
        'startUnifiedMatch': pb.ReqStartUnifiedMatch,
        'cancelUnifiedMatch': pb.ReqCancelUnifiedMatch,
        'fetchGamePointRank': pb.ReqGamePointRank,
        'fetchSelfGamePointRank': pb.ReqGamePointRank,
        'readSNS': pb.ReqReadSNS,
        'replySNS': pb.ReqReplySNS,
        'likeSNS': pb.ReqLikeSNS,
        'digMine': pb.ReqDigMine,
        'fetchLastPrivacy': pb.ReqFetchLastPrivacy,
        'checkPrivacy': pb.ReqCheckPrivacy,
        'responseCaptcha': pb.ReqResponseCaptcha,
        'fetchRPGBattleHistory': pb.ReqFetchRPGBattleHistory,
        'fetchRPGBattleHistoryV2': pb.ReqFetchRPGBattleHistory,
        'receiveRPGRewards': pb.ReqReceiveRPGRewards,
        'receiveRPGReward': pb.ReqReceiveRPGReward,
        'buyArenaTicket': pb.ReqBuyArenaTicket,
        'enterArena': pb.ReqEnterArena,
        'receiveArenaReward': pb.ReqArenaReward,
        'fetchOBToken': pb.ReqFetchOBToken,
        'receiveCharacterRewards': pb.ReqReceiveCharacterRewards,
        'feedActivityFeed': pb.ReqFeedActivityFeed,
        'sendActivityGiftToFriend': pb.ReqSendActivityGiftToFriend,
        'receiveActivityGift': pb.ReqReceiveActivityGift,
        'receiveAllActivityGift': pb.ReqReceiveAllActivityGift,
        'fetchFriendGiftActivityData': pb.ReqFetchFriendGiftActivityData,
        'openPreChestItem': pb.ReqOpenPreChestItem,
        'fetchVoteActivity': pb.ReqFetchVoteActivity,
        'voteActivity': pb.ReqVoteActivity,
        'unlockActivitySpot': pb.ReqUnlockActivitySpot,
        'unlockActivitySpotEnding': pb.ReqUnlockActivitySpotEnding,
        'receiveActivitySpotReward': pb.ReqReceiveActivitySpotReward,
        'deleteAccount': pb.ReqCommon,
        'cancelDeleteAccount': pb.ReqCommon,
        'logReport': pb.ReqLogReport,
        'bindOauth2': pb.ReqBindOauth2,
        'fetchOauth2Info': pb.ReqFetchOauth2,
        'setLoadingImage': pb.ReqSetLoadingImage,
        'fetchShopInterval': pb.ReqCommon,
        'fetchActivityInterval': pb.ReqCommon,
        'fetchRecentFriend': pb.ReqCommon,
        'openGacha': pb.ReqOpenGacha,
        'taskRequest': pb.ReqTaskRequest,
        'simulationActivityTrain': pb.ReqSimulationActivityTrain,
        'fetchSimulationGameRecord': pb.ReqFetchSimulationGameRecord,
        'startSimulationActivityGame': pb.ReqStartSimulationActivityGame,
        'fetchSimulationGameRank': pb.ReqFetchSimulationGameRank,
        'generateCombiningCraft': pb.ReqGenerateCombiningCraft,
        'moveCombiningCraft': pb.ReqMoveCombiningCraft,
        'combiningRecycleCraft': pb.ReqCombiningRecycleCraft,
        'recoverCombiningRecycle': pb.ReqRecoverCombiningRecycle,
        'finishCombiningOrder': pb.ReqFinishCombiningOrder,
    }
    _res = {
        'fetchConnectionInfo': pb.ResConnectionInfo,
        'fetchQueueInfo': pb.ResFetchQueueInfo,
        'cancelQueue': pb.ResCommon,
        'openidCheck': pb.ResOauth2Check,
        'signup': pb.ResSignupAccount,
        'login': pb.ResLogin,
        'fetchInfo': pb.ResFetchInfo,
        'loginSuccess': pb.ResCommon,
        'emailLogin': pb.ResLogin,
        'oauth2Auth': pb.ResOauth2Auth,
        'oauth2Check': pb.ResOauth2Check,
        'oauth2Signup': pb.ResOauth2Signup,
        'oauth2Login': pb.ResLogin,
        'dmmPreLogin': pb.ResDMMPreLogin,
        'createPhoneVerifyCode': pb.ResCommon,
        'createEmailVerifyCode': pb.ResCommon,
        'verfifyCodeForSecure': pb.ResVerfiyCodeForSecure,
        'bindPhoneNumber': pb.ResCommon,
        'unbindPhoneNumber': pb.ResCommon,
        'fetchPhoneLoginBind': pb.ResFetchPhoneLoginBind,
        'createPhoneLoginBind': pb.ResCommon,
        'bindEmail': pb.ResCommon,
        'modifyPassword': pb.ResCommon,
        'bindAccount': pb.ResCommon,
        'logout': pb.ResLogout,
        'heatbeat': pb.ResCommon,
        'loginBeat': pb.ResCommon,
        'createNickname': pb.ResCommon,
        'modifyNickname': pb.ResCommon,
        'modifyBirthday': pb.ResCommon,
        'fetchRoom': pb.ResSelfRoom,
        'createRoom': pb.ResCreateRoom,
        'joinRoom': pb.ResJoinRoom,
        'leaveRoom': pb.ResCommon,
        'readyPlay': pb.ResCommon,
        'dressingStatus': pb.ResCommon,
        'startRoom': pb.ResCommon,
        'kickPlayer': pb.ResCommon,
        'modifyRoom': pb.ResCommon,
        'matchGame': pb.ResCommon,
        'cancelMatch': pb.ResCommon,
        'fetchAccountInfo': pb.ResAccountInfo,
        'changeAvatar': pb.ResCommon,
        'receiveVersionReward': pb.ResCommon,
        'fetchAccountStatisticInfo': pb.ResAccountStatisticInfo,
        'fetchAccountChallengeRankInfo': pb.ResAccountChallengeRankInfo,
        'fetchAccountCharacterInfo': pb.ResAccountCharacterInfo,
        'shopPurchase': pb.ResShopPurchase,
        'fetchGameRecord': pb.ResGameRecord,
        'readGameRecord': pb.ResCommon,
        'fetchGameRecordList': pb.ResGameRecordList,
        'fetchCollectedGameRecordList': pb.ResCollectedGameRecordList,
        'fetchGameRecordsDetail': pb.ResGameRecordsDetail,
        'addCollectedGameRecord': pb.ResAddCollectedGameRecord,
        'removeCollectedGameRecord': pb.ResRemoveCollectedGameRecord,
        'changeCollectedGameRecordRemarks': pb.ResChangeCollectedGameRecordRemarks,
        'fetchLevelLeaderboard': pb.ResLevelLeaderboard,
        'fetchChallengeLeaderboard': pb.ResChallengeLeaderboard,
        'fetchMutiChallengeLevel': pb.ResMutiChallengeLevel,
        'fetchMultiAccountBrief': pb.ResMultiAccountBrief,
        'fetchFriendList': pb.ResFriendList,
        'fetchFriendApplyList': pb.ResFriendApplyList,
        'applyFriend': pb.ResCommon,
        'handleFriendApply': pb.ResCommon,
        'removeFriend': pb.ResCommon,
        'searchAccountById': pb.ResSearchAccountById,
        'searchAccountByPattern': pb.ResSearchAccountByPattern,
        'fetchAccountState': pb.ResAccountStates,
        'fetchBagInfo': pb.ResBagInfo,
        'useBagItem': pb.ResCommon,
        'openManualItem': pb.ResCommon,
        'openRandomRewardItem': pb.ResOpenRandomRewardItem,
        'openAllRewardItem': pb.ResOpenAllRewardItem,
        'composeShard': pb.ResCommon,
        'fetchAnnouncement': pb.ResAnnouncement,
        'readAnnouncement': pb.ResCommon,
        'fetchMailInfo': pb.ResMailInfo,
        'readMail': pb.ResCommon,
        'deleteMail': pb.ResCommon,
        'takeAttachmentFromMail': pb.ResCommon,
        'receiveAchievementReward': pb.ResReceiveAchievementReward,
        'receiveAchievementGroupReward': pb.ResReceiveAchievementGroupReward,
        'fetchAchievementRate': pb.ResFetchAchievementRate,
        'fetchAchievement': pb.ResAchievement,
        'buyShiLian': pb.ResCommon,
        'matchShiLian': pb.ResCommon,
        'goNextShiLian': pb.ResCommon,
        'updateClientValue': pb.ResCommon,
        'fetchClientValue': pb.ResClientValue,
        'clientMessage': pb.ResCommon,
        'fetchCurrentMatchInfo': pb.ResCurrentMatchInfo,
        'userComplain': pb.ResCommon,
        'fetchReviveCoinInfo': pb.ResReviveCoinInfo,
        'gainReviveCoin': pb.ResCommon,
        'fetchDailyTask': pb.ResDailyTask,
        'refreshDailyTask': pb.ResRefreshDailyTask,
        'useGiftCode': pb.ResUseGiftCode,
        'useSpecialGiftCode': pb.ResUseSpecialGiftCode,
        'fetchTitleList': pb.ResTitleList,
        'useTitle': pb.ResCommon,
        'sendClientMessage': pb.ResCommon,
        'fetchGameLiveInfo': pb.ResGameLiveInfo,
        'fetchGameLiveLeftSegment': pb.ResGameLiveLeftSegment,
        'fetchGameLiveList': pb.ResGameLiveList,
        'fetchCommentSetting': pb.ResCommentSetting,
        'updateCommentSetting': pb.ResCommon,
        'fetchCommentList': pb.ResFetchCommentList,
        'fetchCommentContent': pb.ResFetchCommentContent,
        'leaveComment': pb.ResCommon,
        'deleteComment': pb.ResCommon,
        'updateReadComment': pb.ResCommon,
        'fetchRollingNotice': pb.ReqRollingNotice,
        'fetchServerTime': pb.ResServerTime,
        'fetchPlatformProducts': pb.ResPlatformBillingProducts,
        'cancelGooglePlayOrder': pb.ResCommon,
        'openChest': pb.ResOpenChest,
        'buyFromChestShop': pb.ResBuyFromChestShop,
        'fetchDailySignInInfo': pb.ResDailySignInInfo,
        'doDailySignIn': pb.ResCommon,
        'doActivitySignIn': pb.ResDoActivitySignIn,
        'fetchCharacterInfo': pb.ResCharacterInfo,
        'updateCharacterSort': pb.ResCommon,
        'changeMainCharacter': pb.ResCommon,
        'changeCharacterSkin': pb.ResCommon,
        'changeCharacterView': pb.ResCommon,
        'setHiddenCharacter': pb.ResSetHiddenCharacter,
        'sendGiftToCharacter': pb.ResSendGiftToCharacter,
        'sellItem': pb.ResCommon,
        'fetchCommonView': pb.ResCommonView,
        'changeCommonView': pb.ResCommon,
        'saveCommonViews': pb.ResCommon,
        'fetchCommonViews': pb.ResCommonViews,
        'fetchAllCommonViews': pb.ResAllcommonViews,
        'useCommonView': pb.ResCommon,
        'upgradeCharacter': pb.ResUpgradeCharacter,
        'addFinishedEnding': pb.ResCommon,
        'receiveEndingReward': pb.ResCommon,
        'gameMasterCommand': pb.ResCommon,
        'fetchShopInfo': pb.ResShopInfo,
        'buyFromShop': pb.ResBuyFromShop,
        'buyFromZHP': pb.ResCommon,
        'refreshZHPShop': pb.ResRefreshZHPShop,
        'fetchMonthTicketInfo': pb.ResMonthTicketInfo,
        'payMonthTicket': pb.ResPayMonthTicket,
        'exchangeCurrency': pb.ResCommon,
        'exchangeChestStone': pb.ResCommon,
        'exchangeDiamond': pb.ResCommon,
        'fetchServerSettings': pb.ResServerSettings,
        'fetchAccountSettings': pb.ResAccountSettings,
        'updateAccountSettings': pb.ResCommon,
        'fetchModNicknameTime': pb.ResModNicknameTime,
        'createWechatNativeOrder': pb.ResCreateWechatNativeOrder,
        'createWechatAppOrder': pb.ResCreateWechatAppOrder,
        'createAlipayOrder': pb.ResCreateAlipayOrder,
        'createAlipayScanOrder': pb.ResCreateAlipayScanOrder,
        'createAlipayAppOrder': pb.ResCreateAlipayAppOrder,
        'createJPCreditCardOrder': pb.ResCreateJPCreditCardOrder,
        'createJPPaypalOrder': pb.ResCreateJPPaypalOrder,
        'createJPAuOrder': pb.ResCreateJPAuOrder,
        'createJPDocomoOrder': pb.ResCreateJPDocomoOrder,
        'createJPWebMoneyOrder': pb.ResCreateJPWebMoneyOrder,
        'createJPSoftbankOrder': pb.ResCreateJPSoftbankOrder,
        'createJPPayPayOrder': pb.ResCreateJPPayPayOrder,
        'fetchJPCommonCreditCardOrder': pb.ResFetchJPCommonCreditCardOrder,
        'createJPGMOOrder': pb.ResCreateJPGMOOrder,
        'createENPaypalOrder': pb.ResCreateENPaypalOrder,
        'createENMasterCardOrder': pb.ResCreateENMasterCardOrder,
        'createENVisaOrder': pb.ResCreateENVisaOrder,
        'createENJCBOrder': pb.ResCreateENJCBOrder,
        'createENAlipayOrder': pb.ResCreateENAlipayOrder,
        'createKRPaypalOrder': pb.ResCreateKRPaypalOrder,
        'createKRMasterCardOrder': pb.ResCreateKRMasterCardOrder,
        'createKRVisaOrder': pb.ResCreateKRVisaOrder,
        'createKRJCBOrder': pb.ResCreateKRJCBOrder,
        'createKRAlipayOrder': pb.ResCreateKRAlipayOrder,
        'createDMMOrder': pb.ResCreateDmmOrder,
        'createIAPOrder': pb.ResCreateIAPOrder,
        'createSteamOrder': pb.ResCreateSteamOrder,
        'verifySteamOrder': pb.ResCommon,
        'createMyCardAndroidOrder': pb.ResCreateMyCardOrder,
        'createMyCardWebOrder': pb.ResCreateMyCardOrder,
        'createPaypalOrder': pb.ResCreatePaypalOrder,
        'createXsollaOrder': pb.ResCreateXsollaOrder,
        'verifyMyCardOrder': pb.ResCommon,
        'verificationIAPOrder': pb.ResVerificationIAPOrder,
        'createYostarSDKOrder': pb.ResCreateYostarOrder,
        'createBillingOrder': pb.ResCreateBillingOrder,
        'solveGooglePlayOrder': pb.ResCommon,
        'solveGooglePayOrderV3': pb.ResCommon,
        'deliverAA32Order': pb.ResCommon,
        'fetchMisc': pb.ResMisc,
        'modifySignature': pb.ResCommon,
        'fetchIDCardInfo': pb.ResIDCardInfo,
        'updateIDCardInfo': pb.ResCommon,
        'fetchVipReward': pb.ResVipReward,
        'gainVipReward': pb.ResCommon,
        'fetchRefundOrder': pb.ResFetchRefundOrder,
        'fetchCustomizedContestList': pb.ResFetchCustomizedContestList,
        'fetchCustomizedContestExtendInfo': pb.ResFetchCustomizedContestExtendInfo,
        'fetchCustomizedContestAuthInfo': pb.ResFetchCustomizedContestAuthInfo,
        'enterCustomizedContest': pb.ResEnterCustomizedContest,
        'leaveCustomizedContest': pb.ResCommon,
        'fetchCustomizedContestOnlineInfo': pb.ResFetchCustomizedContestOnlineInfo,
        'fetchCustomizedContestByContestId': pb.ResFetchCustomizedContestByContestId,
        'startCustomizedContest': pb.ResCommon,
        'stopCustomizedContest': pb.ResCommon,
        'joinCustomizedContestChatRoom': pb.ResJoinCustomizedContestChatRoom,
        'leaveCustomizedContestChatRoom': pb.ResCommon,
        'sayChatMessage': pb.ResCommon,
        'fetchCustomizedContestGameRecords': pb.ResFetchCustomizedContestGameRecords,
        'fetchCustomizedContestGameLiveList': pb.ResFetchCustomizedContestGameLiveList,
        'followCustomizedContest': pb.ResCommon,
        'unfollowCustomizedContest': pb.ResCommon,
        'fetchActivityList': pb.ResActivityList,
        'fetchAccountActivityData': pb.ResAccountActivityData,
        'exchangeActivityItem': pb.ResExchangeActivityItem,
        'completeActivityTask': pb.ResCommon,
        'completeActivityFlipTask': pb.ResCommon,
        'completePeriodActivityTask': pb.ResCommon,
        'completePeriodActivityTaskBatch': pb.ResCommon,
        'completeRandomActivityTask': pb.ResCommon,
        'receiveActivityFlipTask': pb.ResReceiveActivityFlipTask,
        'completeSegmentTaskReward': pb.ResCompleteSegmentTaskReward,
        'fetchActivityFlipInfo': pb.ResFetchActivityFlipInfo,
        'gainAccumulatedPointActivityReward': pb.ResCommon,
        'gainMultiPointActivityReward': pb.ResCommon,
        'fetchRankPointLeaderboard': pb.ResFetchRankPointLeaderboard,
        'gainRankPointReward': pb.ResCommon,
        'richmanActivityNextMove': pb.ResRichmanNextMove,
        'richmanAcitivitySpecialMove': pb.ResRichmanNextMove,
        'richmanActivityChestInfo': pb.ResRichmanChestInfo,
        'createGameObserveAuth': pb.ResCreateGameObserveAuth,
        'refreshGameObserveAuth': pb.ResRefreshGameObserveAuth,
        'fetchActivityBuff': pb.ResActivityBuff,
        'upgradeActivityBuff': pb.ResActivityBuff,
        'upgradeActivityLevel': pb.ResUpgradeActivityLevel,
        'receiveUpgradeActivityReward': pb.ResReceiveUpgradeActivityReward,
        'upgradeChallenge': pb.ResUpgradeChallenge,
        'refreshChallenge': pb.ResRefreshChallenge,
        'fetchChallengeInfo': pb.ResFetchChallengeInfo,
        'forceCompleteChallengeTask': pb.ResCommon,
        'fetchChallengeSeason': pb.ResChallengeSeasonInfo,
        'receiveChallengeRankReward': pb.ResReceiveChallengeRankReward,
        'fetchABMatchInfo': pb.ResFetchABMatch,
        'buyInABMatch': pb.ResCommon,
        'receiveABMatchReward': pb.ResCommon,
        'quitABMatch': pb.ResCommon,
        'startUnifiedMatch': pb.ResCommon,
        'cancelUnifiedMatch': pb.ResCommon,
        'fetchGamePointRank': pb.ResGamePointRank,
        'fetchSelfGamePointRank': pb.ResFetchSelfGamePointRank,
        'readSNS': pb.ResReadSNS,
        'replySNS': pb.ResReplySNS,
        'likeSNS': pb.ResLikeSNS,
        'digMine': pb.ResDigMine,
        'fetchLastPrivacy': pb.ResFetchLastPrivacy,
        'checkPrivacy': pb.ResCommon,
        'responseCaptcha': pb.ResCommon,
        'fetchRPGBattleHistory': pb.ResFetchRPGBattleHistory,
        'fetchRPGBattleHistoryV2': pb.ResFetchRPGBattleHistoryV2,
        'receiveRPGRewards': pb.ResReceiveRPGRewards,
        'receiveRPGReward': pb.ResReceiveRPGRewards,
        'buyArenaTicket': pb.ResCommon,
        'enterArena': pb.ResCommon,
        'receiveArenaReward': pb.ResArenaReward,
        'fetchOBToken': pb.ResFetchOBToken,
        'receiveCharacterRewards': pb.ResReceiveCharacterRewards,
        'feedActivityFeed': pb.ResFeedActivityFeed,
        'sendActivityGiftToFriend': pb.ResSendActivityGiftToFriend,
        'receiveActivityGift': pb.ResCommon,
        'receiveAllActivityGift': pb.ResReceiveAllActivityGift,
        'fetchFriendGiftActivityData': pb.ResFetchFriendGiftActivityData,
        'openPreChestItem': pb.ResOpenPreChestItem,
        'fetchVoteActivity': pb.ResFetchVoteActivity,
        'voteActivity': pb.ResVoteActivity,
        'unlockActivitySpot': pb.ResCommon,
        'unlockActivitySpotEnding': pb.ResCommon,
        'receiveActivitySpotReward': pb.ResReceiveActivitySpotReward,
        'deleteAccount': pb.ResDeleteAccount,
        'cancelDeleteAccount': pb.ResCommon,
        'logReport': pb.ResCommon,
        'bindOauth2': pb.ResCommon,
        'fetchOauth2Info': pb.ResFetchOauth2,
        'setLoadingImage': pb.ResCommon,
        'fetchShopInterval': pb.ResFetchShopInterval,
        'fetchActivityInterval': pb.ResFetchActivityInterval,
        'fetchRecentFriend': pb.ResFetchrecentFriend,
        'openGacha': pb.ResOpenGacha,
        'taskRequest': pb.ResCommon,
        'simulationActivityTrain': pb.ResSimulationActivityTrain,
        'fetchSimulationGameRecord': pb.ResFetchSimulationGameRecord,
        'startSimulationActivityGame': pb.ResStartSimulationActivityGame,
        'fetchSimulationGameRank': pb.ResFetchSimulationGameRank,
        'generateCombiningCraft': pb.ResGenerateCombiningCraft,
        'moveCombiningCraft': pb.ResMoveCombiningCraft,
        'combiningRecycleCraft': pb.ResCombiningRecycleCraft,
        'recoverCombiningRecycle': pb.ResRecoverCombiningRecycle,
        'finishCombiningOrder': pb.ResFinishCombiningOrder,
    }

    def get_package_name(self):
        return 'lq'

    def get_service_name(self):
        return 'Lobby'

    def get_req_class(self, method):
        return Lobby._req[method]

    def get_res_class(self, method):
        return Lobby._res[method]

    async def fetch_connection_info(self, req):
        return await self.call_method('fetchConnectionInfo', req)

    async def fetch_queue_info(self, req):
        return await self.call_method('fetchQueueInfo', req)

    async def cancel_queue(self, req):
        return await self.call_method('cancelQueue', req)

    async def openid_check(self, req):
        return await self.call_method('openidCheck', req)

    async def signup(self, req):
        return await self.call_method('signup', req)

    async def login(self, req):
        return await self.call_method('login', req)

    async def fetch_info(self, req):
        return await self.call_method('fetchInfo', req)

    async def login_success(self, req):
        return await self.call_method('loginSuccess', req)

    async def email_login(self, req):
        return await self.call_method('emailLogin', req)

    async def oauth2_auth(self, req):
        return await self.call_method('oauth2Auth', req)

    async def oauth2_check(self, req):
        return await self.call_method('oauth2Check', req)

    async def oauth2_signup(self, req):
        return await self.call_method('oauth2Signup', req)

    async def oauth2_login(self, req):
        return await self.call_method('oauth2Login', req)

    async def dmm_pre_login(self, req):
        return await self.call_method('dmmPreLogin', req)

    async def create_phone_verify_code(self, req):
        return await self.call_method('createPhoneVerifyCode', req)

    async def create_email_verify_code(self, req):
        return await self.call_method('createEmailVerifyCode', req)

    async def verfify_code_for_secure(self, req):
        return await self.call_method('verfifyCodeForSecure', req)

    async def bind_phone_number(self, req):
        return await self.call_method('bindPhoneNumber', req)

    async def unbind_phone_number(self, req):
        return await self.call_method('unbindPhoneNumber', req)

    async def fetch_phone_login_bind(self, req):
        return await self.call_method('fetchPhoneLoginBind', req)

    async def create_phone_login_bind(self, req):
        return await self.call_method('createPhoneLoginBind', req)

    async def bind_email(self, req):
        return await self.call_method('bindEmail', req)

    async def modify_password(self, req):
        return await self.call_method('modifyPassword', req)

    async def bind_account(self, req):
        return await self.call_method('bindAccount', req)

    async def logout(self, req):
        return await self.call_method('logout', req)

    async def heatbeat(self, req):
        return await self.call_method('heatbeat', req)

    async def login_beat(self, req):
        return await self.call_method('loginBeat', req)

    async def create_nickname(self, req):
        return await self.call_method('createNickname', req)

    async def modify_nickname(self, req):
        return await self.call_method('modifyNickname', req)

    async def modify_birthday(self, req):
        return await self.call_method('modifyBirthday', req)

    async def fetch_room(self, req):
        return await self.call_method('fetchRoom', req)

    async def create_room(self, req):
        return await self.call_method('createRoom', req)

    async def join_room(self, req):
        return await self.call_method('joinRoom', req)

    async def leave_room(self, req):
        return await self.call_method('leaveRoom', req)

    async def ready_play(self, req):
        return await self.call_method('readyPlay', req)

    async def dressing_status(self, req):
        return await self.call_method('dressingStatus', req)

    async def start_room(self, req):
        return await self.call_method('startRoom', req)

    async def kick_player(self, req):
        return await self.call_method('kickPlayer', req)

    async def modify_room(self, req):
        return await self.call_method('modifyRoom', req)

    async def match_game(self, req):
        return await self.call_method('matchGame', req)

    async def cancel_match(self, req):
        return await self.call_method('cancelMatch', req)

    async def fetch_account_info(self, req):
        return await self.call_method('fetchAccountInfo', req)

    async def change_avatar(self, req):
        return await self.call_method('changeAvatar', req)

    async def receive_version_reward(self, req):
        return await self.call_method('receiveVersionReward', req)

    async def fetch_account_statistic_info(self, req):
        return await self.call_method('fetchAccountStatisticInfo', req)

    async def fetch_account_challenge_rank_info(self, req):
        return await self.call_method('fetchAccountChallengeRankInfo', req)

    async def fetch_account_character_info(self, req):
        return await self.call_method('fetchAccountCharacterInfo', req)

    async def shop_purchase(self, req):
        return await self.call_method('shopPurchase', req)

    async def fetch_game_record(self, req):
        return await self.call_method('fetchGameRecord', req)

    async def read_game_record(self, req):
        return await self.call_method('readGameRecord', req)

    async def fetch_game_record_list(self, req):
        return await self.call_method('fetchGameRecordList', req)

    async def fetch_collected_game_record_list(self, req):
        return await self.call_method('fetchCollectedGameRecordList', req)

    async def fetch_game_records_detail(self, req):
        return await self.call_method('fetchGameRecordsDetail', req)

    async def add_collected_game_record(self, req):
        return await self.call_method('addCollectedGameRecord', req)

    async def remove_collected_game_record(self, req):
        return await self.call_method('removeCollectedGameRecord', req)

    async def change_collected_game_record_remarks(self, req):
        return await self.call_method('changeCollectedGameRecordRemarks', req)

    async def fetch_level_leaderboard(self, req):
        return await self.call_method('fetchLevelLeaderboard', req)

    async def fetch_challenge_leaderboard(self, req):
        return await self.call_method('fetchChallengeLeaderboard', req)

    async def fetch_muti_challenge_level(self, req):
        return await self.call_method('fetchMutiChallengeLevel', req)

    async def fetch_multi_account_brief(self, req):
        return await self.call_method('fetchMultiAccountBrief', req)

    async def fetch_friend_list(self, req):
        return await self.call_method('fetchFriendList', req)

    async def fetch_friend_apply_list(self, req):
        return await self.call_method('fetchFriendApplyList', req)

    async def apply_friend(self, req):
        return await self.call_method('applyFriend', req)

    async def handle_friend_apply(self, req):
        return await self.call_method('handleFriendApply', req)

    async def remove_friend(self, req):
        return await self.call_method('removeFriend', req)

    async def search_account_by_id(self, req):
        return await self.call_method('searchAccountById', req)

    async def search_account_by_pattern(self, req):
        return await self.call_method('searchAccountByPattern', req)

    async def fetch_account_state(self, req):
        return await self.call_method('fetchAccountState', req)

    async def fetch_bag_info(self, req):
        return await self.call_method('fetchBagInfo', req)

    async def use_bag_item(self, req):
        return await self.call_method('useBagItem', req)

    async def open_manual_item(self, req):
        return await self.call_method('openManualItem', req)

    async def open_random_reward_item(self, req):
        return await self.call_method('openRandomRewardItem', req)

    async def open_all_reward_item(self, req):
        return await self.call_method('openAllRewardItem', req)

    async def compose_shard(self, req):
        return await self.call_method('composeShard', req)

    async def fetch_announcement(self, req):
        return await self.call_method('fetchAnnouncement', req)

    async def read_announcement(self, req):
        return await self.call_method('readAnnouncement', req)

    async def fetch_mail_info(self, req):
        return await self.call_method('fetchMailInfo', req)

    async def read_mail(self, req):
        return await self.call_method('readMail', req)

    async def delete_mail(self, req):
        return await self.call_method('deleteMail', req)

    async def take_attachment_from_mail(self, req):
        return await self.call_method('takeAttachmentFromMail', req)

    async def receive_achievement_reward(self, req):
        return await self.call_method('receiveAchievementReward', req)

    async def receive_achievement_group_reward(self, req):
        return await self.call_method('receiveAchievementGroupReward', req)

    async def fetch_achievement_rate(self, req):
        return await self.call_method('fetchAchievementRate', req)

    async def fetch_achievement(self, req):
        return await self.call_method('fetchAchievement', req)

    async def buy_shi_lian(self, req):
        return await self.call_method('buyShiLian', req)

    async def match_shi_lian(self, req):
        return await self.call_method('matchShiLian', req)

    async def go_next_shi_lian(self, req):
        return await self.call_method('goNextShiLian', req)

    async def update_client_value(self, req):
        return await self.call_method('updateClientValue', req)

    async def fetch_client_value(self, req):
        return await self.call_method('fetchClientValue', req)

    async def client_message(self, req):
        return await self.call_method('clientMessage', req)

    async def fetch_current_match_info(self, req):
        return await self.call_method('fetchCurrentMatchInfo', req)

    async def user_complain(self, req):
        return await self.call_method('userComplain', req)

    async def fetch_revive_coin_info(self, req):
        return await self.call_method('fetchReviveCoinInfo', req)

    async def gain_revive_coin(self, req):
        return await self.call_method('gainReviveCoin', req)

    async def fetch_daily_task(self, req):
        return await self.call_method('fetchDailyTask', req)

    async def refresh_daily_task(self, req):
        return await self.call_method('refreshDailyTask', req)

    async def use_gift_code(self, req):
        return await self.call_method('useGiftCode', req)

    async def use_special_gift_code(self, req):
        return await self.call_method('useSpecialGiftCode', req)

    async def fetch_title_list(self, req):
        return await self.call_method('fetchTitleList', req)

    async def use_title(self, req):
        return await self.call_method('useTitle', req)

    async def send_client_message(self, req):
        return await self.call_method('sendClientMessage', req)

    async def fetch_game_live_info(self, req):
        return await self.call_method('fetchGameLiveInfo', req)

    async def fetch_game_live_left_segment(self, req):
        return await self.call_method('fetchGameLiveLeftSegment', req)

    async def fetch_game_live_list(self, req):
        return await self.call_method('fetchGameLiveList', req)

    async def fetch_comment_setting(self, req):
        return await self.call_method('fetchCommentSetting', req)

    async def update_comment_setting(self, req):
        return await self.call_method('updateCommentSetting', req)

    async def fetch_comment_list(self, req):
        return await self.call_method('fetchCommentList', req)

    async def fetch_comment_content(self, req):
        return await self.call_method('fetchCommentContent', req)

    async def leave_comment(self, req):
        return await self.call_method('leaveComment', req)

    async def delete_comment(self, req):
        return await self.call_method('deleteComment', req)

    async def update_read_comment(self, req):
        return await self.call_method('updateReadComment', req)

    async def fetch_rolling_notice(self, req):
        return await self.call_method('fetchRollingNotice', req)

    async def fetch_server_time(self, req):
        return await self.call_method('fetchServerTime', req)

    async def fetch_platform_products(self, req):
        return await self.call_method('fetchPlatformProducts', req)

    async def cancel_google_play_order(self, req):
        return await self.call_method('cancelGooglePlayOrder', req)

    async def open_chest(self, req):
        return await self.call_method('openChest', req)

    async def buy_from_chest_shop(self, req):
        return await self.call_method('buyFromChestShop', req)

    async def fetch_daily_sign_in_info(self, req):
        return await self.call_method('fetchDailySignInInfo', req)

    async def do_daily_sign_in(self, req):
        return await self.call_method('doDailySignIn', req)

    async def do_activity_sign_in(self, req):
        return await self.call_method('doActivitySignIn', req)

    async def fetch_character_info(self, req):
        return await self.call_method('fetchCharacterInfo', req)

    async def update_character_sort(self, req):
        return await self.call_method('updateCharacterSort', req)

    async def change_main_character(self, req):
        return await self.call_method('changeMainCharacter', req)

    async def change_character_skin(self, req):
        return await self.call_method('changeCharacterSkin', req)

    async def change_character_view(self, req):
        return await self.call_method('changeCharacterView', req)

    async def set_hidden_character(self, req):
        return await self.call_method('setHiddenCharacter', req)

    async def send_gift_to_character(self, req):
        return await self.call_method('sendGiftToCharacter', req)

    async def sell_item(self, req):
        return await self.call_method('sellItem', req)

    async def fetch_common_view(self, req):
        return await self.call_method('fetchCommonView', req)

    async def change_common_view(self, req):
        return await self.call_method('changeCommonView', req)

    async def save_common_views(self, req):
        return await self.call_method('saveCommonViews', req)

    async def fetch_common_views(self, req):
        return await self.call_method('fetchCommonViews', req)

    async def fetch_all_common_views(self, req):
        return await self.call_method('fetchAllCommonViews', req)

    async def use_common_view(self, req):
        return await self.call_method('useCommonView', req)

    async def upgrade_character(self, req):
        return await self.call_method('upgradeCharacter', req)

    async def add_finished_ending(self, req):
        return await self.call_method('addFinishedEnding', req)

    async def receive_ending_reward(self, req):
        return await self.call_method('receiveEndingReward', req)

    async def game_master_command(self, req):
        return await self.call_method('gameMasterCommand', req)

    async def fetch_shop_info(self, req):
        return await self.call_method('fetchShopInfo', req)

    async def buy_from_shop(self, req):
        return await self.call_method('buyFromShop', req)

    async def buy_from_zhp(self, req):
        return await self.call_method('buyFromZHP', req)

    async def refresh_zhp_shop(self, req):
        return await self.call_method('refreshZHPShop', req)

    async def fetch_month_ticket_info(self, req):
        return await self.call_method('fetchMonthTicketInfo', req)

    async def pay_month_ticket(self, req):
        return await self.call_method('payMonthTicket', req)

    async def exchange_currency(self, req):
        return await self.call_method('exchangeCurrency', req)

    async def exchange_chest_stone(self, req):
        return await self.call_method('exchangeChestStone', req)

    async def exchange_diamond(self, req):
        return await self.call_method('exchangeDiamond', req)

    async def fetch_server_settings(self, req):
        return await self.call_method('fetchServerSettings', req)

    async def fetch_account_settings(self, req):
        return await self.call_method('fetchAccountSettings', req)

    async def update_account_settings(self, req):
        return await self.call_method('updateAccountSettings', req)

    async def fetch_mod_nickname_time(self, req):
        return await self.call_method('fetchModNicknameTime', req)

    async def create_wechat_native_order(self, req):
        return await self.call_method('createWechatNativeOrder', req)

    async def create_wechat_app_order(self, req):
        return await self.call_method('createWechatAppOrder', req)

    async def create_alipay_order(self, req):
        return await self.call_method('createAlipayOrder', req)

    async def create_alipay_scan_order(self, req):
        return await self.call_method('createAlipayScanOrder', req)

    async def create_alipay_app_order(self, req):
        return await self.call_method('createAlipayAppOrder', req)

    async def create_jp_credit_card_order(self, req):
        return await self.call_method('createJPCreditCardOrder', req)

    async def create_jp_paypal_order(self, req):
        return await self.call_method('createJPPaypalOrder', req)

    async def create_jp_au_order(self, req):
        return await self.call_method('createJPAuOrder', req)

    async def create_jp_docomo_order(self, req):
        return await self.call_method('createJPDocomoOrder', req)

    async def create_jp_web_money_order(self, req):
        return await self.call_method('createJPWebMoneyOrder', req)

    async def create_jp_softbank_order(self, req):
        return await self.call_method('createJPSoftbankOrder', req)

    async def create_jp_pay_pay_order(self, req):
        return await self.call_method('createJPPayPayOrder', req)

    async def fetch_jp_common_credit_card_order(self, req):
        return await self.call_method('fetchJPCommonCreditCardOrder', req)

    async def create_jpgmo_order(self, req):
        return await self.call_method('createJPGMOOrder', req)

    async def create_en_paypal_order(self, req):
        return await self.call_method('createENPaypalOrder', req)

    async def create_en_master_card_order(self, req):
        return await self.call_method('createENMasterCardOrder', req)

    async def create_en_visa_order(self, req):
        return await self.call_method('createENVisaOrder', req)

    async def create_enjcb_order(self, req):
        return await self.call_method('createENJCBOrder', req)

    async def create_en_alipay_order(self, req):
        return await self.call_method('createENAlipayOrder', req)

    async def create_kr_paypal_order(self, req):
        return await self.call_method('createKRPaypalOrder', req)

    async def create_kr_master_card_order(self, req):
        return await self.call_method('createKRMasterCardOrder', req)

    async def create_kr_visa_order(self, req):
        return await self.call_method('createKRVisaOrder', req)

    async def create_krjcb_order(self, req):
        return await self.call_method('createKRJCBOrder', req)

    async def create_kr_alipay_order(self, req):
        return await self.call_method('createKRAlipayOrder', req)

    async def create_dmm_order(self, req):
        return await self.call_method('createDMMOrder', req)

    async def create_iap_order(self, req):
        return await self.call_method('createIAPOrder', req)

    async def create_steam_order(self, req):
        return await self.call_method('createSteamOrder', req)

    async def verify_steam_order(self, req):
        return await self.call_method('verifySteamOrder', req)

    async def create_my_card_android_order(self, req):
        return await self.call_method('createMyCardAndroidOrder', req)

    async def create_my_card_web_order(self, req):
        return await self.call_method('createMyCardWebOrder', req)

    async def create_paypal_order(self, req):
        return await self.call_method('createPaypalOrder', req)

    async def create_xsolla_order(self, req):
        return await self.call_method('createXsollaOrder', req)

    async def verify_my_card_order(self, req):
        return await self.call_method('verifyMyCardOrder', req)

    async def verification_iap_order(self, req):
        return await self.call_method('verificationIAPOrder', req)

    async def create_yostar_sdk_order(self, req):
        return await self.call_method('createYostarSDKOrder', req)

    async def create_billing_order(self, req):
        return await self.call_method('createBillingOrder', req)

    async def solve_google_play_order(self, req):
        return await self.call_method('solveGooglePlayOrder', req)

    async def solve_google_pay_order_v3(self, req):
        return await self.call_method('solveGooglePayOrderV3', req)

    async def deliver_aa32_order(self, req):
        return await self.call_method('deliverAA32Order', req)

    async def fetch_misc(self, req):
        return await self.call_method('fetchMisc', req)

    async def modify_signature(self, req):
        return await self.call_method('modifySignature', req)

    async def fetch_id_card_info(self, req):
        return await self.call_method('fetchIDCardInfo', req)

    async def update_id_card_info(self, req):
        return await self.call_method('updateIDCardInfo', req)

    async def fetch_vip_reward(self, req):
        return await self.call_method('fetchVipReward', req)

    async def gain_vip_reward(self, req):
        return await self.call_method('gainVipReward', req)

    async def fetch_refund_order(self, req):
        return await self.call_method('fetchRefundOrder', req)

    async def fetch_customized_contest_list(self, req):
        return await self.call_method('fetchCustomizedContestList', req)

    async def fetch_customized_contest_extend_info(self, req):
        return await self.call_method('fetchCustomizedContestExtendInfo', req)

    async def fetch_customized_contest_auth_info(self, req):
        return await self.call_method('fetchCustomizedContestAuthInfo', req)

    async def enter_customized_contest(self, req):
        return await self.call_method('enterCustomizedContest', req)

    async def leave_customized_contest(self, req):
        return await self.call_method('leaveCustomizedContest', req)

    async def fetch_customized_contest_online_info(self, req):
        return await self.call_method('fetchCustomizedContestOnlineInfo', req)

    async def fetch_customized_contest_by_contest_id(self, req):
        return await self.call_method('fetchCustomizedContestByContestId', req)

    async def start_customized_contest(self, req):
        return await self.call_method('startCustomizedContest', req)

    async def stop_customized_contest(self, req):
        return await self.call_method('stopCustomizedContest', req)

    async def join_customized_contest_chat_room(self, req):
        return await self.call_method('joinCustomizedContestChatRoom', req)

    async def leave_customized_contest_chat_room(self, req):
        return await self.call_method('leaveCustomizedContestChatRoom', req)

    async def say_chat_message(self, req):
        return await self.call_method('sayChatMessage', req)

    async def fetch_customized_contest_game_records(self, req):
        return await self.call_method('fetchCustomizedContestGameRecords', req)

    async def fetch_customized_contest_game_live_list(self, req):
        return await self.call_method('fetchCustomizedContestGameLiveList', req)

    async def follow_customized_contest(self, req):
        return await self.call_method('followCustomizedContest', req)

    async def unfollow_customized_contest(self, req):
        return await self.call_method('unfollowCustomizedContest', req)

    async def fetch_activity_list(self, req):
        return await self.call_method('fetchActivityList', req)

    async def fetch_account_activity_data(self, req):
        return await self.call_method('fetchAccountActivityData', req)

    async def exchange_activity_item(self, req):
        return await self.call_method('exchangeActivityItem', req)

    async def complete_activity_task(self, req):
        return await self.call_method('completeActivityTask', req)

    async def complete_activity_flip_task(self, req):
        return await self.call_method('completeActivityFlipTask', req)

    async def complete_period_activity_task(self, req):
        return await self.call_method('completePeriodActivityTask', req)

    async def complete_period_activity_task_batch(self, req):
        return await self.call_method('completePeriodActivityTaskBatch', req)

    async def complete_random_activity_task(self, req):
        return await self.call_method('completeRandomActivityTask', req)

    async def receive_activity_flip_task(self, req):
        return await self.call_method('receiveActivityFlipTask', req)

    async def complete_segment_task_reward(self, req):
        return await self.call_method('completeSegmentTaskReward', req)

    async def fetch_activity_flip_info(self, req):
        return await self.call_method('fetchActivityFlipInfo', req)

    async def gain_accumulated_point_activity_reward(self, req):
        return await self.call_method('gainAccumulatedPointActivityReward', req)

    async def gain_multi_point_activity_reward(self, req):
        return await self.call_method('gainMultiPointActivityReward', req)

    async def fetch_rank_point_leaderboard(self, req):
        return await self.call_method('fetchRankPointLeaderboard', req)

    async def gain_rank_point_reward(self, req):
        return await self.call_method('gainRankPointReward', req)

    async def richman_activity_next_move(self, req):
        return await self.call_method('richmanActivityNextMove', req)

    async def richman_acitivity_special_move(self, req):
        return await self.call_method('richmanAcitivitySpecialMove', req)

    async def richman_activity_chest_info(self, req):
        return await self.call_method('richmanActivityChestInfo', req)

    async def create_game_observe_auth(self, req):
        return await self.call_method('createGameObserveAuth', req)

    async def refresh_game_observe_auth(self, req):
        return await self.call_method('refreshGameObserveAuth', req)

    async def fetch_activity_buff(self, req):
        return await self.call_method('fetchActivityBuff', req)

    async def upgrade_activity_buff(self, req):
        return await self.call_method('upgradeActivityBuff', req)

    async def upgrade_activity_level(self, req):
        return await self.call_method('upgradeActivityLevel', req)

    async def receive_upgrade_activity_reward(self, req):
        return await self.call_method('receiveUpgradeActivityReward', req)

    async def upgrade_challenge(self, req):
        return await self.call_method('upgradeChallenge', req)

    async def refresh_challenge(self, req):
        return await self.call_method('refreshChallenge', req)

    async def fetch_challenge_info(self, req):
        return await self.call_method('fetchChallengeInfo', req)

    async def force_complete_challenge_task(self, req):
        return await self.call_method('forceCompleteChallengeTask', req)

    async def fetch_challenge_season(self, req):
        return await self.call_method('fetchChallengeSeason', req)

    async def receive_challenge_rank_reward(self, req):
        return await self.call_method('receiveChallengeRankReward', req)

    async def fetch_ab_match_info(self, req):
        return await self.call_method('fetchABMatchInfo', req)

    async def buy_in_ab_match(self, req):
        return await self.call_method('buyInABMatch', req)

    async def receive_ab_match_reward(self, req):
        return await self.call_method('receiveABMatchReward', req)

    async def quit_ab_match(self, req):
        return await self.call_method('quitABMatch', req)

    async def start_unified_match(self, req):
        return await self.call_method('startUnifiedMatch', req)

    async def cancel_unified_match(self, req):
        return await self.call_method('cancelUnifiedMatch', req)

    async def fetch_game_point_rank(self, req):
        return await self.call_method('fetchGamePointRank', req)

    async def fetch_self_game_point_rank(self, req):
        return await self.call_method('fetchSelfGamePointRank', req)

    async def read_sns(self, req):
        return await self.call_method('readSNS', req)

    async def reply_sns(self, req):
        return await self.call_method('replySNS', req)

    async def like_sns(self, req):
        return await self.call_method('likeSNS', req)

    async def dig_mine(self, req):
        return await self.call_method('digMine', req)

    async def fetch_last_privacy(self, req):
        return await self.call_method('fetchLastPrivacy', req)

    async def check_privacy(self, req):
        return await self.call_method('checkPrivacy', req)

    async def response_captcha(self, req):
        return await self.call_method('responseCaptcha', req)

    async def fetch_rpg_battle_history(self, req):
        return await self.call_method('fetchRPGBattleHistory', req)

    async def fetch_rpg_battle_history_v2(self, req):
        return await self.call_method('fetchRPGBattleHistoryV2', req)

    async def receive_rpg_rewards(self, req):
        return await self.call_method('receiveRPGRewards', req)

    async def receive_rpg_reward(self, req):
        return await self.call_method('receiveRPGReward', req)

    async def buy_arena_ticket(self, req):
        return await self.call_method('buyArenaTicket', req)

    async def enter_arena(self, req):
        return await self.call_method('enterArena', req)

    async def receive_arena_reward(self, req):
        return await self.call_method('receiveArenaReward', req)

    async def fetch_ob_token(self, req):
        return await self.call_method('fetchOBToken', req)

    async def receive_character_rewards(self, req):
        return await self.call_method('receiveCharacterRewards', req)

    async def feed_activity_feed(self, req):
        return await self.call_method('feedActivityFeed', req)

    async def send_activity_gift_to_friend(self, req):
        return await self.call_method('sendActivityGiftToFriend', req)

    async def receive_activity_gift(self, req):
        return await self.call_method('receiveActivityGift', req)

    async def receive_all_activity_gift(self, req):
        return await self.call_method('receiveAllActivityGift', req)

    async def fetch_friend_gift_activity_data(self, req):
        return await self.call_method('fetchFriendGiftActivityData', req)

    async def open_pre_chest_item(self, req):
        return await self.call_method('openPreChestItem', req)

    async def fetch_vote_activity(self, req):
        return await self.call_method('fetchVoteActivity', req)

    async def vote_activity(self, req):
        return await self.call_method('voteActivity', req)

    async def unlock_activity_spot(self, req):
        return await self.call_method('unlockActivitySpot', req)

    async def unlock_activity_spot_ending(self, req):
        return await self.call_method('unlockActivitySpotEnding', req)

    async def receive_activity_spot_reward(self, req):
        return await self.call_method('receiveActivitySpotReward', req)

    async def delete_account(self, req):
        return await self.call_method('deleteAccount', req)

    async def cancel_delete_account(self, req):
        return await self.call_method('cancelDeleteAccount', req)

    async def log_report(self, req):
        return await self.call_method('logReport', req)

    async def bind_oauth2(self, req):
        return await self.call_method('bindOauth2', req)

    async def fetch_oauth2_info(self, req):
        return await self.call_method('fetchOauth2Info', req)

    async def set_loading_image(self, req):
        return await self.call_method('setLoadingImage', req)

    async def fetch_shop_interval(self, req):
        return await self.call_method('fetchShopInterval', req)

    async def fetch_activity_interval(self, req):
        return await self.call_method('fetchActivityInterval', req)

    async def fetch_recent_friend(self, req):
        return await self.call_method('fetchRecentFriend', req)

    async def open_gacha(self, req):
        return await self.call_method('openGacha', req)

    async def task_request(self, req):
        return await self.call_method('taskRequest', req)

    async def simulation_activity_train(self, req):
        return await self.call_method('simulationActivityTrain', req)

    async def fetch_simulation_game_record(self, req):
        return await self.call_method('fetchSimulationGameRecord', req)

    async def start_simulation_activity_game(self, req):
        return await self.call_method('startSimulationActivityGame', req)

    async def fetch_simulation_game_rank(self, req):
        return await self.call_method('fetchSimulationGameRank', req)

    async def generate_combining_craft(self, req):
        return await self.call_method('generateCombiningCraft', req)

    async def move_combining_craft(self, req):
        return await self.call_method('moveCombiningCraft', req)

    async def combining_recycle_craft(self, req):
        return await self.call_method('combiningRecycleCraft', req)

    async def recover_combining_recycle(self, req):
        return await self.call_method('recoverCombiningRecycle', req)

    async def finish_combining_order(self, req):
        return await self.call_method('finishCombiningOrder', req)


class FastTest(MSRPCService):
    version = None
    
    _req = {
        'authGame': pb.ReqAuthGame,
        'enterGame': pb.ReqCommon,
        'syncGame': pb.ReqSyncGame,
        'finishSyncGame': pb.ReqCommon,
        'terminateGame': pb.ReqCommon,
        'inputOperation': pb.ReqSelfOperation,
        'inputChiPengGang': pb.ReqChiPengGang,
        'confirmNewRound': pb.ReqCommon,
        'broadcastInGame': pb.ReqBroadcastInGame,
        'inputGameGMCommand': pb.ReqGMCommandInGaming,
        'fetchGamePlayerState': pb.ReqCommon,
        'checkNetworkDelay': pb.ReqCommon,
        'clearLeaving': pb.ReqCommon,
        'voteGameEnd': pb.ReqVoteGameEnd,
        'authObserve': pb.ReqAuthObserve,
        'startObserve': pb.ReqCommon,
        'stopObserve': pb.ReqCommon,
    }
    _res = {
        'authGame': pb.ResAuthGame,
        'enterGame': pb.ResEnterGame,
        'syncGame': pb.ResSyncGame,
        'finishSyncGame': pb.ResCommon,
        'terminateGame': pb.ResCommon,
        'inputOperation': pb.ResCommon,
        'inputChiPengGang': pb.ResCommon,
        'confirmNewRound': pb.ResCommon,
        'broadcastInGame': pb.ResCommon,
        'inputGameGMCommand': pb.ResCommon,
        'fetchGamePlayerState': pb.ResGamePlayerState,
        'checkNetworkDelay': pb.ResCommon,
        'clearLeaving': pb.ResCommon,
        'voteGameEnd': pb.ResGameEndVote,
        'authObserve': pb.ResCommon,
        'startObserve': pb.ResStartObserve,
        'stopObserve': pb.ResCommon,
    }

    def get_package_name(self):
        return 'lq'

    def get_service_name(self):
        return 'FastTest'

    def get_req_class(self, method):
        return FastTest._req[method]

    def get_res_class(self, method):
        return FastTest._res[method]

    async def auth_game(self, req):
        return await self.call_method('authGame', req)

    async def enter_game(self, req):
        return await self.call_method('enterGame', req)

    async def sync_game(self, req):
        return await self.call_method('syncGame', req)

    async def finish_sync_game(self, req):
        return await self.call_method('finishSyncGame', req)

    async def terminate_game(self, req):
        return await self.call_method('terminateGame', req)

    async def input_operation(self, req):
        return await self.call_method('inputOperation', req)

    async def input_chi_peng_gang(self, req):
        return await self.call_method('inputChiPengGang', req)

    async def confirm_new_round(self, req):
        return await self.call_method('confirmNewRound', req)

    async def broadcast_in_game(self, req):
        return await self.call_method('broadcastInGame', req)

    async def input_game_gm_command(self, req):
        return await self.call_method('inputGameGMCommand', req)

    async def fetch_game_player_state(self, req):
        return await self.call_method('fetchGamePlayerState', req)

    async def check_network_delay(self, req):
        return await self.call_method('checkNetworkDelay', req)

    async def clear_leaving(self, req):
        return await self.call_method('clearLeaving', req)

    async def vote_game_end(self, req):
        return await self.call_method('voteGameEnd', req)

    async def auth_observe(self, req):
        return await self.call_method('authObserve', req)

    async def start_observe(self, req):
        return await self.call_method('startObserve', req)

    async def stop_observe(self, req):
        return await self.call_method('stopObserve', req)
