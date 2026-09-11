"""Root-only physical integration probe. Requires a managed active user kmscon PID
and an independent root watchdog that restores the original VT after this test.
It duplicates already-open descriptors, then triggers the trusted VT attention path.
It never reads input event contents or credential data.
"""
import os,ctypes,fcntl,json,time,glob,sys
assert os.geteuid() == 0, 'root diagnostic only' 
libc=ctypes.CDLL(None,use_errno=True);libdrm=ctypes.CDLL('libdrm.so.2')
pid=int(sys.argv[1])
assert os.stat(f"/proc/{pid}").st_uid > 0
assert open(f"/proc/{pid}/comm").read().strip() == "kmscon"
pidfd=libc.syscall(434,pid,0);assert pidfd>=0
fds={}
for source in glob.glob(f'/proc/{pid}/fd/*'):
    target=os.readlink(source)
    if target.startswith('/dev/input/event') or target=='/dev/dri/card0':
        fd=libc.syscall(438,pidfd,int(source.rsplit('/',1)[-1]),0)
        if fd<0:raise OSError(ctypes.get_errno(),'pidfd_getfd')
        fds[target]=fd
assert any(k.startswith('/dev/input/') for k in fds)
def inspect():
    result={}
    for target,fd in fds.items():
        if target=='/dev/dri/card0':result[target]={'master':bool(libdrm.drmIsMaster(fd))}
        else:
            try:buf=bytearray(256);fcntl.ioctl(fd,0x81004506,buf,True);result[target]={'readable':True}
            except OSError as e:result[target]={'readable':False,'errno':e.errno}
    return result
before=inspect()
console=os.open('/dev/tty0',os.O_RDWR|os.O_NOCTTY)
fcntl.ioctl(console,0x5606,8)
os.close(console)
time.sleep(2)
after=inspect()
print(json.dumps({'before':before,'after':after}))
assert before['/dev/dri/card0']['master'] and not after['/dev/dri/card0']['master']
assert all(not state['readable'] for name,state in after.items() if name.startswith('/dev/input/'))
for fd in fds.values():os.close(fd)
os.close(pidfd)
print('Retained input and DRM descriptors revoked after trusted handover')
