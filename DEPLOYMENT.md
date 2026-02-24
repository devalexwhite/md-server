# Production deployment (Docker + Caddy)

This repo ships a production-oriented `docker-compose.yml` that runs:

- `md-server` (Rust) on an internal Docker network only
- `caddy` as the public edge reverse proxy (ports 80/443)

## 1) Prepare the host

- Install Docker Engine + Docker Compose plugin.
- Make sure Docker starts on boot:

```bash
sudo systemctl enable --now docker
```

## 2) Put the project in a stable location

Example:

```bash
sudo mkdir -p /opt/md-server
sudo rsync -a --delete ./ /opt/md-server/
```

Create your site content directory on the host:

```bash
sudo mkdir -p /opt/md-server/www
```

## 3) Configure the domain

Create `/opt/md-server/.env`:

```bash
DOMAIN=example.com
```

Caddy will automatically provision/renew a TLS certificate once DNS points at this host.

## 4) Start the stack

```bash
cd /opt/md-server
sudo docker compose up -d --build
```

## 5) Enable crash/boot recovery

The containers have `restart: unless-stopped`, so they come back after crashes and after Docker daemon restarts.

For an additional “start on boot” guard, install the included systemd unit:

```bash
sudo cp /opt/md-server/deploy/systemd/md-server-compose.service /etc/systemd/system/md-server-compose.service
sudo systemctl daemon-reload
sudo systemctl enable --now md-server-compose.service
```

## 6) Firewall (recommended)

Allow only SSH + HTTP + HTTPS. Example with UFW:

```bash
sudo ufw allow OpenSSH
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw enable
```

## Operations

- View logs:

```bash
docker compose logs -f
```

- Restart:

```bash
docker compose restart
```

- Update Caddy / rebuild server:

```bash
docker compose pull caddy
docker compose up -d --build
```
