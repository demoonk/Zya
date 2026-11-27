import sys

sys.stdout.reconfigure(encoding="utf-8")

launcher = r"abrakadabra/target/release/z_single_launcher.exe"
app = r"tools/z.bin"

data = open(launcher, "rb").read()
payload = open(app, "rb").read()

print("launcher size:", len(data))
print("app payload size:", len(payload))
print("app embedded verbatim:", payload in data)
print("PE header of payload:", data.find(payload[:64]))
