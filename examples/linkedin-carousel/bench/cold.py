import subprocess, time, statistics, os, sys, resource
B = os.path.dirname(os.path.abspath(__file__))
R = os.path.abspath(os.path.join(B, "..", "..", ".."))
cases = {
  "pdfcn (CLI release)": ([f"{R}/target/release/pdfcn","build",f"{R}/examples/invoice.haml","-d",f"{R}/examples/invoice.json","-o",f"{B}/c1.pdf"], {}),
  "react-pdf · Helvetica": (["node",f"{B}/once.mjs",f"{B}/c2.pdf"], {}),
  "react-pdf · Inter": (["node",f"{B}/once.mjs",f"{B}/c3.pdf"], {"FONT":"inter"}),
}
N=20
for name,(cmd,env) in cases.items():
    e=dict(os.environ, **env)
    subprocess.run(cmd, env=e, capture_output=True, check=True)  # warm the page cache once
    ts=[]; peak=0
    for _ in range(N):
        t=time.perf_counter(); subprocess.run(cmd, env=e, capture_output=True, check=True); ts.append((time.perf_counter()-t)*1000)
    peak=resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss/1024
    ts.sort()
    print(f"{name:24s} median {statistics.median(ts):6.0f} ms   p95 {ts[int(N*0.95)-1]:6.0f} ms   min {ts[0]:5.0f} ms")
