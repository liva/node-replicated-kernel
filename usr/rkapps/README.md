# rkapps

A wrapper to easily build applications already ported to run on top of rumpkernel for bespin
(i.e., the following repo <https://github.com/rumpkernel/rumprun-packages>)

If we want to invoke a build manually:

## redis

```bash
cd redis
cd target/x86_64-bespin-none/debug/build/rkapps-$HASH/out/redis
export RUMPRUN_TOOLCHAIN_TUPLE=x86_64-rumprun-netbsd
export PATH=`realpath ../../../rumpkernel-$HASH/out/rumprun/bin`:$PATH
make
rumprun-bake bespin_generic redis.out ./bin/redis-server
```

## memcached

```bash
cd "target/x86_64-bespin-none/release/build/rkapps-8a4ead00329ed64e/out/memcached"
PATH=target/x86_64-bespin-none/release/build/rumpkernel-934f79a93edbe559/out/rumprun/bin:$PATH RUMPRUN_TOOLCHAIN_TUPLE=x86_64-rumprun-netbsd make -j 12
PATH=target/x86_64-bespin-none/release/build/rumpkernel-934f79a93edbe559/out/rumprun/bin:$PATH RUMPRUN_TOOLCHAIN_TUPLE=x86_64-rumprun-netbsd rumprun-bake bespin_generic ../../../../memcached.bin build/memcached
```
