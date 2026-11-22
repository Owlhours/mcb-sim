#!/usr/bin/env python3
# census of blocks over the reference builds
import sys, zlib, struct, glob, collections
names=[];
for l in open('work/reference/mc/states.tsv'):
    f=l.split('\t')
    if f[0]=='id': continue
    names.append(f[1])
def load(h):
    d=zlib.decompress(open(f'work/corpus/cache/{h[:2]}/{h}.mcb','rb').read())
    magic,sx,sy,sz,unk,dv=struct.unpack('>iiiiqi',d[:28]); n=struct.unpack('>H',d[28:30])[0]; p=30+n
    cnt=collections.Counter(); tot=sx*sy*sz; filled=0
    while filled<tot:
        run=0; sh=0
        while True:
            b=d[p]; p+=1; run|=(b&127)<<sh; sh+=7
            if b<128: break
        sid=(d[p]<<8)|d[p+1]; p+=2
        cnt[sid]+=run; filled+=run
    return sx,sy,sz,cnt
blocks=collections.Counter(); builds=collections.Counter()
hs=[]
for f in glob.glob('work/reference/results/f6b21f4-*.tsv'):
    hs+= [l.split('\t')[0] for l in open(f)]
for h in hs:
    sx,sy,sz,c=load(h)
    seen=set()
    for sid,n in c.items():
        if sid==0: continue
        blocks[names[sid]]+=n; seen.add(names[sid])
    for s in seen: builds[s]+=1
print(len(hs),'builds',len(blocks),'distinct blocks', sum(blocks.values()),'cells')
for b,n in builds.most_common(): print(b,n,blocks[b])
