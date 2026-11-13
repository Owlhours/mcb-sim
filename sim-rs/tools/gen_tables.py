#!/usr/bin/env python3
"""Builds /work/data/{blocks,tags,items}.tsv from the reference material (states.tsv, decompiled Blocks.java,
the 26.1 data reports for defaults/stack sizes, tag JSON from the server jar)."""
import re, json, os, sys, zipfile, io, collections
sys.path.insert(0, os.path.dirname(__file__))
from parse_blocks import parse
REF='/work/reference/mc'
OUT='/work/data'
rows=[l.rstrip('\n').split('\t') for l in open(f'{REF}/states.tsv')]
hdr=rows[0]; rows=rows[1:]
H={n:i for i,n in enumerate(hdr)}
blocks=[]  # name, first, n, props(list of (name,[values]))
for r in rows:
    sid=int(r[0]); name=r[1]
    if not blocks or blocks[-1]['name']!=name:
        blocks.append({'name':name,'first':sid,'n':0,'props':[], 'item':r[H['item']]})
    b=blocks[-1]; b['n']+=1
    if r[2]:
        kv=[p.split('=') for p in r[2].split(',')]
        if not b['props']: b['props']=[(k,[]) for k,_ in kv]
        for (k,vals),(k2,v) in zip(b['props'],kv):
            assert k==k2
            if v not in vals: vals.append(v)
# check cartesian enumeration: first property most significant
for b in blocks:
    n=1
    for _,v in b['props']: n*=len(v)
    assert n==b['n'], b
    # verify ordering on a few states
    for off in (0,b['n']-1,b['n']//2):
        r=rows[b['first']+off]; kv=dict(p.split('=') for p in r[2].split(',')) if r[2] else {}
        idx=0
        for k,vals in b['props']: idx=idx*len(vals)+vals.index(kv[k])
        assert idx==off,(b['name'],off,idx)
# ---- classes
fields,txt=parse()
def cls_of_body(body):
    head=re.match(r'\w+',body).group(0)
    if head in('registerLegacyStair','registerStair'): return 'StairBlock'
    if head=='registerSlab': return 'SlabBlock'
    if head=='registerWall': return 'WallBlock'
    m=re.search(r'new (\w+)\(',body) or re.search(r'(\w+)::new',body)
    return m.group(1) if m else 'Block'
cls={k.lower():cls_of_body(v) for k,v in fields.items()}
body={k.lower():v for k,v in fields.items()}
COLORS='white orange magenta light_blue yellow lime pink gray light_gray cyan purple blue brown green red black'.split()
COLOR_RULES={'bed':'BedBlock','wool':'Block','stained_glass':'StainedGlassBlock','terracotta':'Block','stained_glass_pane':'StainedGlassPaneBlock','carpet':'WoolCarpetBlock',
 'banner':'BannerBlock','wall_banner':'WallBannerBlock','shulker_box':'ShulkerBoxBlock','glazed_terracotta':'GlazedTerracottaBlock','concrete':'Block',
 'concrete_powder':'ConcretePowderBlock','candle':'CandleBlock','candle_cake':'CandleCakeBlock'}
COPPER_RULES=[('copper_bars','IronBarsBlock'),('copper_chain','ChainBlock'),('copper_lantern','LanternBlock'),('cut_copper_stairs','StairBlock'),('cut_copper_slab','SlabBlock'),
 ('copper_door','DoorBlock'),('copper_trapdoor','TrapDoorBlock'),('copper_grate','WaterloggedTransparentBlock'),('copper_bulb','CopperBulbBlock'),('copper_chest','ChestBlock'),
 ('copper_golem_statue','CopperGolemStatueBlock'),('lightning_rod','LightningRodBlock'),('chiseled_copper','Block'),('cut_copper','Block'),('copper_block','Block'),('copper','Block')]
ALIAS={'TintedParticleLeavesBlock':'LeavesBlock','UntintedParticleLeavesBlock':'LeavesBlock','MangroveLeavesBlock':'LeavesBlock','DropExperienceBlock':'Block','InfestedBlock':'Block',
 'InfestedRotatedPillarBlock':'RotatedPillarBlock','SandBlock':'FallingBlock','ColoredFallingBlock':'FallingBlock','HayBlock':'RotatedPillarBlock','TransparentBlock':'Block','HalfTransparentBlock':'Block',
 'StainedGlassBlock':'Block','TintedGlassBlock':'Block','AmethystBlock':'Block','PoweredBlock':'PoweredBlock','TrappedChestBlock':'ChestBlock','WitherRoseBlock':'FlowerBlock','EyeblossomBlock':'FlowerBlock',
 'WitherSkullBlock':'SkullBlock','PlayerHeadBlock':'SkullBlock','WitherWallSkullBlock':'WallSkullBlock','PlayerWallHeadBlock':'WallSkullBlock','PiglinWallSkullBlock':'WallSkullBlock',
 'StainedGlassPaneBlock':'IronBarsBlock','WoolCarpetBlock':'CarpetBlock','SmokerBlock':'FurnaceBlock','BlastFurnaceBlock':'FurnaceBlock','DropperBlock':'DispenserBlock',
 'CarrotBlock':'CropBlock','PotatoBlock':'CropBlock','BeetrootBlock':'CropBlock','RedstoneWallTorchBlock':'RedstoneWallTorchBlock','WaterloggedTransparentBlock':'Block',
 'BaseCoralPlantBlock':'CoralPlantBlock','BaseCoralFanBlock':'CoralFanBlock','BaseCoralWallFanBlock':'CoralWallFanBlock','NyliumBlock':'Block','MyceliumBlock':'SnowyBlock','GrassBlock':'SnowyBlock',
 'NetherrackBlock':'Block','RootedDirtBlock':'Block','SpongeBlock':'Block','WetSpongeBlock':'Block','RedStoneOreBlock':'Block','BuddingAmethystBlock':'Block','CryingObsidianBlock':'Block',
 'MagmaBlock':'Block','IceBlock':'Block','SculkBlock':'Block','SculkCatalystBlock':'Block','PumpkinBlock':'Block','MangroveRootsBlock':'Block','TallDryGrassBlock':'DryVegetationBlock','ShortDryGrassBlock':'DryVegetationBlock',
 'SoulFireBlock':'FireBlock','CalibratedSculkSensorBlock':'SculkSensorBlock','TestBlock':'Block','TestInstanceBlock':'GameMasterBlock','StructureBlock':'GameMasterBlock','JigsawBlock':'JigsawBlock','CommandBlock':'CommandBlock'}
def classify(name):
    c=None
    if name in cls: c=cls[name]
    else:
        for col in COLORS:
            if name.startswith(col+'_') and name[len(col)+1:] in COLOR_RULES: c=COLOR_RULES[name[len(col)+1:]]
        if c is None and ('copper' in name or 'lightning_rod' in name):
            for suf,k in COPPER_RULES:
                if name.endswith(suf): c=k; break
        if c is None and name.startswith('potted_'): c='FlowerPotBlock'
    assert c is not None, name
    return ALIAS.get(c,c)
# ---- property flags from Blocks.java bodies (direct only, with one level of copy resolution)
def flag(name, pat, depth=0):
    b=body.get(name)
    if b is None: return None
    m=re.search(pat,b)
    if m: return m.group(1) if m.groups() else '1'
    m=re.search(r'of(?:Full|Legacy)Copy\((\w+)\)',b)
    if m and depth<4: return flag(m.group(1).lower(),pat,depth+1)
    return None
# ---- defaults from the 26.1 report (relative offset), fallback by signature
old={e['name']:e for e in json.load(open(f'{REF}/data/blocks.json'))}
sig_default={}
for b in blocks:
    o=old.get(b['name'])
    b['class']=classify(b['name'])
    if o and o['maxStateId']-o['minStateId']+1==b['n']:
        b['default']=b['first']+o['defaultState']-o['minStateId']
        sig_default[(b['class'],tuple((k,tuple(v)) for k,v in b['props']))]=o['defaultState']-o['minStateId']
fb=[]
for b in blocks:
    if 'default' in b: continue
    key=(b['class'],tuple((k,tuple(v)) for k,v in b['props']))
    if key in sig_default: b['default']=b['first']+sig_default[key]
    else:
        idx=0
        for k,vals in b['props']:
            i=vals.index('false') if 'false' in vals else 0
            idx=idx*len(vals)+i
        b['default']=b['first']+idx; fb.append(b['name'])
# the Java version's own dump of default states (reference/java-extra), where available, is the authority
extra_path='/work/reference/java-extra/states-extra.tsv'
if os.path.exists(extra_path):
    ex=[l.rstrip('\n').split('\t') for l in open(extra_path)]
    ci=ex[0].index('defaultStateId'); ex=ex[1:]
    changed=[]
    for b in blocks:
        d=int(ex[b['first']][ci])
        if d!=b['default']: changed.append(b['name']); b['default']=d
    print('default states corrected from states-extra.tsv:',changed,file=sys.stderr)
print('default-state fallbacks (heuristic):',fb,file=sys.stderr)
os.makedirs(OUT,exist_ok=True)
with open(f'{OUT}/blocks.tsv','w') as f:
    for i,b in enumerate(blocks):
        off=flag(b['name'],r'offsetType\(BlockBehaviour\.OffsetType\.(\w+)\)') or 'NONE'
        push=flag(b['name'],r'pushReaction\(PushReaction\.(\w+)\)') or 'NORMAL'
        f.write('\t'.join([str(i),b['name'],b['class'],str(b['first']),str(b['n']),str(b['default']),off,push,b['item'],';'.join(k+'='+'|'.join(v) for k,v in b['props'])])+'\n')
# ---- kinds enum for Rust
kinds=sorted({b['class'] for b in blocks})
with open('/work/sim/src/kinds.rs','w') as f:
    f.write('// generated by tools/gen_tables.py - do not edit\n#[allow(dead_code)]\n#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]\n#[repr(u16)]\npub enum Kind {\n')
    for k in kinds: f.write(f'    {k},\n')
    f.write('}\npub const KIND_NAMES: &[(&str, Kind)] = &[\n')
    for k in kinds: f.write(f'    ("{k}", Kind::{k}),\n')
    f.write('];\n')
# ---- tags
z=zipfile.ZipFile(io.BytesIO(zipfile.ZipFile(f'{REF}/server-26.2.jar').read('META-INF/versions/26.2/server-26.2.jar')))
def load_tags(kind):
    raw={}
    pre=f'data/minecraft/tags/{kind}/'
    for n in z.namelist():
        if n.startswith(pre) and n.endswith('.json'):
            raw[n[len(pre):-5]]=json.loads(z.read(n))['values']
    res={}
    def resolve(t):
        if t in res: return res[t]
        out=[]
        for v in raw[t]:
            if isinstance(v,dict): v=v['id']
            if v.startswith('#'): out+=resolve(v[1:].replace('minecraft:',''))
            else: out.append(v.replace('minecraft:',''))
        res[t]=out; return out
    for t in raw: resolve(t)
    return res
with open(f'{OUT}/tags.tsv','w') as f:
    for kind in ('block','item'):
        for t,vals in sorted(load_tags(kind).items()):
            f.write(f'{kind}\t{t}\t'+','.join(dict.fromkeys(vals))+'\n')
# ---- items
items=json.load(open(f'{REF}/data/items.json'))
stack={e['name']:e['stackSize'] for e in items}
names=list(dict.fromkeys(b['item'] for b in blocks))
with open(f'{OUT}/items.tsv','w') as f:
    for n in names:
        if n=='air': continue
        f.write(f"{n}\t{stack.get(n,64)}\n")
miss=[n for n in names if n not in stack]
print('items without a stack size in the 26.1 report (64 assumed):',miss,file=sys.stderr)
print(len(blocks),'blocks',len(kinds),'kinds',file=sys.stderr)
# ---- property / value constants for Rust
pnames=sorted({k for b in blocks for k,_ in b['props']})
vnames=sorted({v for b in blocks for _,vals in b['props'] for v in vals if not v.isdigit()})
with open('/work/sim/src/props.rs','w') as f:
    f.write('// generated by tools/gen_tables.py - do not edit\n#![allow(dead_code)]\n')
    for i,p in enumerate(pnames): f.write(f'pub const P_{p.upper()}: u8 = {i};\n')
    f.write(f'pub const N_PROPS: usize = {len(pnames)};\n')
    f.write('pub const PROP_NAMES: &[&str] = &['+', '.join(f'"{p}"' for p in pnames)+'];\n')
    f.write('// value codes: integers are their own code (0..=31); named values follow\n')
    for i,v in enumerate(vnames): f.write(f'pub const V_{v.upper()}: u8 = {32+i};\n')
    f.write('pub const VALUE_NAMES: &[&str] = &['+', '.join(f'"{v}"' for v in vnames)+'];\n')
print(len(pnames),'props',len(vnames),'named values',file=sys.stderr)
