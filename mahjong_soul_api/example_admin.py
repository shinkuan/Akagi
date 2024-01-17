import asyncio
import hashlib
import hmac
import logging
import random
import uuid
from optparse import OptionParser

import aiohttp

from ms_tournament.base import MSRPCChannel
from ms_tournament.rpc import CustomizedContestManagerApi
import ms_tournament.protocol_admin_pb2 as pb


logging.basicConfig(
    level=logging.INFO, format="%(asctime)s %(levelname)s: %(message)s", datefmt="%Y-%m-%d %H:%M:%S"
)

MS_HOST = "https://mahjongsoul.tournament.yo-star.com"
MS_MANAGER_API_URL = "https://mjusgs.mahjongsoul.com:7988"

async def main():
    """
    Login to the EN server with OAuth2 access token and get tournament list.
    """
    parser = OptionParser()
    parser.add_option("-t", "--token", type="string", help="Access token for connect.")

    opts, _ = parser.parse_args()
    access_token = opts.token

    if not access_token:
        parser.error("Access token cant be empty")

    manager_api, channel = await connect()
    await login(manager_api, access_token)
    await load_tournaments_list(manager_api)
    await channel.close()


async def connect():
    async with aiohttp.ClientSession() as session:
    	async with session.get("{}/api/customized_contest/random".format(MS_MANAGER_API_URL)) as res:
            servers = await res.json()
            endpoint_gate = servers['servers'][0]
            endpoint = "wss://{}/".format(endpoint_gate)

    logging.info(f"Chosen endpoint: {endpoint}")
    channel = MSRPCChannel(endpoint)

    manager_api = CustomizedContestManagerApi(channel)

    await channel.connect(MS_HOST)
    logging.info("Connection was established")

    return manager_api, channel


async def login(manager_api, access_token):
    logging.info("")
    logging.info("Login with OAuth2 access token to manager panel")

    req = pb.ReqContestManageOauth2Login()
    req.type = 7
    req.access_token = access_token
    req.reconnect = False

    res = await manager_api.oauth2_login_contest_manager(req)
    token = res.access_token
    nickname = res.nickname
    account_id = res.account_id
    if not token:
        logging.error("Login Error:")
        logging.error(res)
        return False
    logging.info("Login succesfull!")
    logging.info("###################################################")
    logging.info(f"access token: {token}")
    logging.info(f"account_id: {account_id}")
    logging.info(f"nickname: {nickname}")
    logging.info("###################################################")
    logging.info("")
    return True

async def load_tournaments_list(manager_api):
    logging.info("Loading tournament list...")

    req = pb.ReqCommon()
    res = await manager_api.fetch_related_contest_list(req)
    tournaments_count = len(res.contests)
    logging.info(f"found tournaments : {tournaments_count}")

    for i in range(0, tournaments_count):
        logging.info("") 
        logging.info(f"unique_id: {res.contests[i].unique_id}") 
        logging.info(f"contest_id: {res.contests[i].contest_id}")
        logging.info(f"contest_name: {res.contests[i].contest_name}")

    return True

if __name__ == "__main__":
    asyncio.run(main())
