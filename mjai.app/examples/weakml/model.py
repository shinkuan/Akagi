import json
from typing import Optional
from loguru import logger

import numpy as np
import torch
from torch import nn
from torch.nn import functional as F
from torch.distributions import Normal, Categorical
from libriichi.mjai import Bot
from libriichi.consts import OBS_SHAPE, ORACLE_OBS_SHAPE, ACTION_SPACE, GRP_SIZE


@torch.jit.script
def apply_masks(actions, masks, fill: float = -1e9):
    fill = torch.tensor(fill, dtype=actions.dtype, device=actions.device)
    return torch.where(masks, actions, fill)


class ChannelAttention(nn.Module):
    def __init__(self, channels, ratio=16):
        super().__init__()
        self.avg = nn.AdaptiveAvgPool1d(1)
        self.max = nn.AdaptiveMaxPool1d(1)
        self.shared_mlp = nn.Sequential(
            nn.Linear(channels, channels // ratio),
            nn.ReLU(inplace=True),
            nn.Linear(channels // ratio, channels),
        )

    def forward(self, x):
        avg_out = self.avg(x).squeeze(-1)
        max_out = self.max(x).squeeze(-1)
        avg_out = self.shared_mlp(avg_out)
        max_out = self.shared_mlp(max_out)
        out = torch.sigmoid(avg_out + max_out).unsqueeze(-1)
        return out


class ResBlock(nn.Module):
    def __init__(self, channels, enable_bn, bn_momentum):
        super().__init__()
        tch_bn_momentum = None
        if bn_momentum is not None:
            tch_bn_momentum = 1 - bn_momentum

        self.res_unit = nn.Sequential(
            nn.Conv1d(channels, channels, kernel_size=3, padding=1, bias=not enable_bn),
            nn.BatchNorm1d(channels, momentum=tch_bn_momentum) if enable_bn else nn.Identity(),
            nn.ReLU(inplace=True),
            nn.Conv1d(channels, channels, kernel_size=3, padding=1, bias=not enable_bn),
            nn.BatchNorm1d(channels, momentum=tch_bn_momentum) if enable_bn else nn.Identity(),
        )
        self.ca = ChannelAttention(channels)

    def forward(self, x):
        out = self.res_unit(x)
        out = self.ca(out) * out
        out += x
        out = F.relu(out, inplace=True)
        return out


class ResNet(nn.Module):
    def __init__(self, in_channels, conv_channels, num_blocks, enable_bn, bn_momentum):
        super().__init__()
        tch_bn_momentum = None
        if bn_momentum is not None:
            tch_bn_momentum = 1 - bn_momentum

        blocks = []
        for _ in range(num_blocks):
            blocks.append(ResBlock(conv_channels, enable_bn=enable_bn, bn_momentum=bn_momentum))

        self.net = nn.Sequential(
            nn.Conv1d(in_channels, conv_channels, kernel_size=3, padding=1, bias=not enable_bn),
            nn.BatchNorm1d(conv_channels, momentum=tch_bn_momentum) if enable_bn else nn.Identity(),
            nn.ReLU(inplace=True),
            *blocks,
            nn.Conv1d(conv_channels, 32, kernel_size=3, padding=1),
            nn.ReLU(inplace=True),
            nn.Flatten(),
            nn.Linear(32 * 34, 1024),
        )

    def forward(self, x):
        return self.net(x)


class Brain(nn.Module):
    def __init__(self, is_oracle, conv_channels, num_blocks, enable_bn, bn_momentum):
        super().__init__()
        self.is_oracle = is_oracle
        in_channels = OBS_SHAPE[0]
        if is_oracle:
            in_channels += ORACLE_OBS_SHAPE[0]

        if bn_momentum == 0:
            bn_momentum = None
        self.encoder = ResNet(
            in_channels,
            conv_channels = conv_channels,
            num_blocks = num_blocks,
            enable_bn = enable_bn,
            bn_momentum = bn_momentum,
        )

        self.latent_net = nn.Sequential(
            nn.Linear(1024, 512),
            nn.ReLU(inplace=True),
        )
        self.mu_head = nn.Linear(512, 512)
        self.logsig_head = nn.Linear(512, 512)

        # when True, never updates running stats, weights and bias and always use EMA or CMA
        self._freeze_bn = False

    def forward(self, obs, invisible_obs: Optional[torch.Tensor] = None):
        if self.is_oracle:
            assert invisible_obs is not None
            obs = torch.cat((obs, invisible_obs), dim=1)

        encoded = self.encoder(obs)
        latent_out = self.latent_net(encoded)
        mu = self.mu_head(latent_out)
        logsig = self.logsig_head(latent_out)
        return mu, logsig

    def train(self, mode=True):
        super().train(mode)
        if self._freeze_bn:
            for module in self.modules():
                if isinstance(module, nn.BatchNorm1d):
                    module.eval()
                    # I don't think this benefits
                    # module.requires_grad_(False)
        return self

    def reset_running_stats(self):
        for module in self.modules():
            if isinstance(module, nn.BatchNorm1d):
                module.reset_running_stats()

    def freeze_bn(self, flag):
        self._freeze_bn = flag
        return self.train(self.training)


class DQN(nn.Module):
    def __init__(self):
        super().__init__()
        self.v_head = nn.Linear(512, 1)
        self.a_head = nn.Linear(512, ACTION_SPACE)

    def forward(self, latent, mask):
        v = self.v_head(latent)
        a = self.a_head(latent)

        a_sum = apply_masks(a, mask, fill=0).sum(-1, keepdim=True)
        mask_sum = mask.sum(-1, keepdim=True)
        a_mean = a_sum / mask_sum
        q = apply_masks(v + a - a_mean, mask)
        return q


class MortalEngine:
    def __init__(
        self,
        brain,
        dqn,
        is_oracle,
        device = None,
        stochastic_latent = False,
        enable_amp = False,
        enable_quick_eval = True,
        enable_rule_based_agari_guard = False,
        name = 'NoName',
        boltzmann_epsilon = 0,
        boltzmann_temp = 1,
        use_obs_extend = True,
    ):
        self.device = device or torch.device('cpu')
        if isinstance(device, str):
            self.device = torch.device(device)

        self.brain = brain.to(self.device).eval()
        self.dqn = dqn.to(self.device).eval()
        self.is_oracle = is_oracle
        self.stochastic_latent = stochastic_latent

        self.enable_amp = enable_amp
        self.enable_quick_eval = enable_quick_eval
        self.enable_rule_based_agari_guard = enable_rule_based_agari_guard
        self.name = name

        self.boltzmann_epsilon = boltzmann_epsilon
        self.boltzmann_temp = boltzmann_temp

    def react_batch(self, obs, masks, invisible_obs):
        with (
            torch.autocast(self.device.type, enabled=self.enable_amp),
            torch.no_grad(),
        ):
            return self._react_batch(obs, masks, invisible_obs)

    def _react_batch(self, obs, masks, invisible_obs):
        obs = torch.as_tensor(np.stack(obs, axis=0), device=self.device)
        masks = torch.as_tensor(np.stack(masks, axis=0), device=self.device)
        if self.is_oracle:
            invisible_obs = torch.as_tensor(np.stack(invisible_obs, axis=0), device=self.device)
        else:
            invisible_obs = None
        batch_size = obs.shape[0]

        mu, logsig = self.brain(obs, invisible_obs)
        if self.stochastic_latent:
            latent = Normal(mu, logsig.exp()).sample()
        else:
            latent = mu
        q_out = self.dqn(latent, masks)

        if self.boltzmann_epsilon > 0:
            is_greedy = torch.rand(batch_size, device=self.device) >= self.boltzmann_epsilon
            logits = apply_masks(q_out / self.boltzmann_temp, masks, fill=-1e9)
            actions = torch.where(
                is_greedy,
                q_out.argmax(-1),
                Categorical(logits=logits).sample(),
            )
        else:
            is_greedy = torch.ones(batch_size, dtype=torch.bool, device=self.device)
            actions = q_out.argmax(-1)

        return actions.tolist(), q_out.tolist(), masks.tolist(), is_greedy.tolist()


def load_model(seat: int) -> Bot:
    device = torch.device('cpu')

    # control_state_file = "./mortal_offline_v6_510k.pth"

    # latest binary model
    control_state_file = "./weakml.pth"
    resnet_configs = dict(
        conv_channels = 192,
        num_blocks = 40,
        enable_bn = True,
        bn_momentum = 0.99
    )

    mortal = Brain(False, **resnet_configs).eval()
    dqn = DQN().eval()
    state = torch.load(control_state_file, map_location=torch.device('cpu'))
    mortal.load_state_dict(state['mortal'])
    dqn.load_state_dict(state['current_dqn'])

    engine = MortalEngine(
        mortal,
        dqn,
        is_oracle = False,
        device = device,
        enable_amp = False,
        enable_quick_eval = False,
        enable_rule_based_agari_guard = True,
        name = 'mortal',
        use_obs_extend = True,
    )

    bot = Bot(engine, seat)
    return bot
