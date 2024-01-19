#!/bin/bash

cd /workspace
sudo /workspace/.pyenv/shims/python mitm.py &
sudo /workspace/.pyenv/shims/python client.py
