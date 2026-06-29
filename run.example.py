import os

user = 'user1'
password = 'passwd123'

remote_path = '/home/logs'
local_path = 'C:/rock/logs'

host = 'xxxx'
port = 22
cmd = f"""cargo run -- -H {host} -p {port} -u {user} -P "{password}"  --remote {remote_path} --local {local_path}"""
os.system(cmd)