# Deployment notes

- Recommened OS to use is _Rocky Linux 9+_.
- Services could be run as docker conteiners, by the way **PostgreSQL** and **bitcoind** MUST BE local native services. *No docker for 2TB database!*


### initial setup

```sh
dnf install epel-release
dnf install tmux git ranger vim btop htop mc firewalld
```

### firewalld setup

```
systemctl start firewalld
firewall-cmd --permanent --add-service=http
firewall-cmd --permanent --add-service=https
firewall-cmd --permanent --add-service=ssh
firewall-cmd --reload
firewall-cmd --permanent --list-all

firewall-cmd --permanent --new-zone=monitoring
firewall-cmd --permanent --zone=monitoring --add-source=<REDACTED>

# node_exporter
firewall-cmd --permanent --zone=monitoring --add-port=9100/tcp
# postgres_exporter
firewall-cmd --permanent --zone=monitoring --add-port=9187/tcp
# process-exporter
firewall-cmd --permanent --zone=monitoring --add-port=9256/tcp

# orbtc-api metrics
firewall-cmd --permanent --zone=monitoring --add-port=9140/tcp
# orbtc-indexer metrics
firewall-cmd --permanent --zone=monitoring --add-port=9141/tcp
# orbtc-runes-indexer metrics
firewall-cmd --permanent --zone=monitoring --add-port=9142/tcp

firewall-cmd --reload
firewall-cmd --zone=monitoring --list-all

```

### postgres 17

```
sudo dnf install -y https://download.postgresql.org/pub/repos/yum/reporpms/EL-9-x86_64/pgdg-redhat-repo-latest.noarch.rpm
sudo dnf -qy module disable postgresql
sudo dnf install -y postgresql17-server postgresql17-contrib libpq

mkdir -p /srv/orbtc/data/postgres
chown postgres:postgres /srv/orbtc/data/postgres


# vim /lib/systemd/system/postgresql-17.service
# -> Environment=PGDATA=/srv/orbtc/data/postgres

systemctl daemon-reload
systemctl enable postgresql-17
systemctl restart postgresql-17

```

Add at the end of the config additional settings:

```
vim data/postgres/postgresql.conf
```

#### postgresql.conf tweaks

```ini
# Made with:
# https://pgtune.leopard.in.ua/

# DB Version: 17
# OS Type: linux
# DB Type: dw
# Total Memory (RAM): 64 GB
# CPUs num: 48
# Connections num: 500
# Data Storage: ssd

max_connections = 500
shared_buffers = 16GB
effective_cache_size = 48GB
maintenance_work_mem = 2GB
checkpoint_completion_target = 0.9
wal_buffers = 16MB
default_statistics_target = 500
random_page_cost = 1.1
effective_io_concurrency = 200
work_mem = 699kB
huge_pages = try
min_wal_size = 4GB
max_wal_size = 16GB
max_worker_processes = 48
max_parallel_workers_per_gather = 24
max_parallel_workers = 48
max_parallel_maintenance_workers = 4

# -----

shared_preload_libraries = 'pg_stat_statements'
compute_query_id = on
pg_stat_statements.max = 10000
pg_stat_statements.track = all
```

#### Create user and databases

```sh
sudo -u postgres psql
```

```sql
CREATE USER dev WITH PASSWORD '<redacted>';
ALTER ROLE dev WITH LOGIN;

CREATE DATABASE orbtc_indexer;
CREATE EXTENSION pg_stat_statements;

GRANT CONNECT ON DATABASE orbtc_indexer TO dev;
GRANT ALL PRIVILEGES ON DATABASE orbtc_indexer TO dev;

```

### node exporter + prometheus exporter

```sh
# https://github.com/prometheus/node_exporter/releases

adduser -M -r -s /sbin/nologin node_exporter

wget https://github.com/prometheus/node_exporter/releases/download/v1.8.2/node_exporter-1.8.2.linux-amd64.tar.gz
tar xzvf ./node_exporter-1.8.2.linux-amd64.tar.gz
mv node_exporter-1.8.2.linux-amd64/node_exporter /usr/local/bin/
chmod +x /usr/local/bin/node_exporter
rm -rf ./node_exporter*

# https://github.com/prometheus-community/postgres_exporter/releases
wget https://github.com/prometheus-community/postgres_exporter/releases/download/v0.16.0/postgres_exporter-0.16.0.linux-amd64.tar.gz
tar xzvf ./postgres_exporter-0.16.0.linux-amd64.tar.gz
cp postgres_exporter-0.16.0.linux-amd64/postgres_exporter /usr/local/bin/
chmod +x /usr/local/bin/postgres_exporter

rm -rf ./postgres_exporter*


# https://github.com/ncabatoff/process-exporter/releases
wget https://github.com/ncabatoff/process-exporter/releases/download/v0.8.5/process-exporter_0.8.5_linux_amd64.rpm
rpm -i process-exporter_0.8.5_linux_amd64.rpm
rm process-exporter_0.8.5_linux_amd64.rpm

```



### bitcoind

```sh
wget https://bitcoincore.org/bin/bitcoin-core-28.1/bitcoin-28.1-x86_64-linux-gnu.tar.gz
tar xzvf ./bitcoin-28.1-x86_64-linux-gnu.tar.gz
cp bitcoin-28.1/bin/bitcoind  ./
rm -rf bitcoin-*

mkdir -p /srv/orbtc/data/bitcoin

chown orbtc:orbtc /srv/orbtc/data/bitcoin
chown orbtc:orbtc ./bitcoind

```
