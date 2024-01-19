FROM ubuntu:22.04

RUN mkdir -p /workspace
WORKDIR /workspace
ENV HOME /workspace

ENV TZ=Asia/Tokyo
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

RUN apt-get -y update && apt-get -y install curl git build-essential libssl-dev zlib1g-dev \
libbz2-dev libreadline-dev libsqlite3-dev curl \
libncursesw5-dev xz-utils tk-dev libxml2-dev libxmlsec1-dev libffi-dev liblzma-dev

ENV PYENV_ROOT $HOME/.pyenv
ENV PATH $PYENV_ROOT/bin:$PATH

RUN curl https://pyenv.run | bash && \
    echo '' >> /workspace/.bashrc && \
    echo 'export PYENV_ROOT="$HOME/.pyenv"' >> /workspace/.bashrc && \
    echo 'export PATH="$PYENV_ROOT/bin:$PATH"' >> /workspace/.bashrc && \
    echo 'eval "$(pyenv init --path)"' >> /workspace/.bashrc && \
    echo 'eval "$(pyenv virtualenv-init -)"' >> /workspace/.bashrc
RUN . /workspace/.bashrc && \
    PYTHON_CONFIGURE_OPTS="--enable-shared" pyenv install 3.10.12 && \
    pyenv global 3.10.12 && \
    pip install -U pip

RUN apt-get -y install wget unzip

# RUN adduser --disabled-password --gecos "" --home /workspace ubuntu &&\
#     echo "ubuntu:ubuntu" | chpasswd &&\
#     chown ubuntu:ubuntu /workspace
# USER ubuntu

ENV HOME /workspace

COPY requirement.txt /workspace/
RUN . /workspace/.bashrc && python -m pip install -r requirement.txt

RUN apt install sudo -y
COPY . /workspace/
RUN . /workspace/.bashrc && chmod +x /workspace/entrypoint.sh

# RUN . /workspace/.bashrc && chown root:root /workspace/client.py
# RUN . /workspace/.bashrc && chmod +x /workspace/client.py

# ENTRYPOINT ["/workspace/entrypoint.sh"]
