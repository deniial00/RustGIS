# Logs every call of Mesa's driCreateNewScreen3 (the EGL setup that called getenv in the
# crashing thread of #1113) with the gdb thread it ran on; thread 1 is Python's main thread.
set pagination off
set confirm off
set breakpoint pending on
set follow-fork-mode parent
set detach-on-fork on
handle SIGPIPE nostop noprint pass
handle SIGUSR1 nostop noprint pass
handle SIGUSR2 nostop noprint pass
handle SIG32 nostop noprint pass
handle SIG33 nostop noprint pass
handle SIG34 nostop noprint pass
handle SIG35 nostop noprint pass
break driCreateNewScreen3
commands
silent
printf "EGL-SETUP driCreateNewScreen3 on gdb thread %d\n", $_thread
continue
end
run
