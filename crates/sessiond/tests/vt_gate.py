import os,fcntl,struct,json,glob,pwd
fd=os.open('/dev/tty0',os.O_RDWR|os.O_NOCTTY)
state=bytearray(6);fcntl.ioctl(fd,0x5603,state,True);assert struct.unpack('HHH',state)[0]==8
fcntl.ioctl(fd,0x5606,9)
fcntl.ioctl(fd,0x5603,state,True);assert struct.unpack('HHH',state)[0]==8
pid=os.fork()
if pid==0:
    user=pwd.getpwnam('tundra-it-user');os.setgroups([]);os.setgid(user.pw_gid);os.setuid(user.pw_uid)
    failures={}
    for name,operation in [('VT_UNLOCKSWITCH',lambda:fcntl.ioctl(fd,0x560c,0)),('TIOCSTI',lambda:fcntl.ioctl(fd,0x5412,b'x'))]:
        try:operation();os._exit(2)
        except OSError as error:failures[name]=error.errno
    for path in ['/dev/uinput']+glob.glob('/dev/input/event*'):
        try:device=os.open(path,os.O_RDONLY|os.O_NONBLOCK);os.close(device);os._exit(3)
        except PermissionError:pass
    print(json.dumps({'passed':True,'root_VT_ACTIVATE_blocked':True,'unprivileged_denials':failures,'raw_input_access_denied':True}),flush=True)
    os._exit(0)
_,status=os.waitpid(pid,0);assert status==0
os.close(fd)
