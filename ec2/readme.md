sudo systemctl daemon-reload
sudo systemctl start rust-chat
sudo systemctl start react-frontend

# Enable auto-start on boot
sudo systemctl enable rust-chat
sudo systemctl enable react-frontend

# For systemd services
journalctl -u rust-chat -f
journalctl -u react-frontend -f

# For PM2
pm2 logs
