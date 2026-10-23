"""Extract observable decision prefixes from local Tenhou sanma XML.zst files.

Stops each round at the first meld or kita; no partial meld decoder is used.
Opponents' concealed hands and draws are hidden in every exported perspective.
Requires zstandard. Public-source logs remain local and are not distributed.
"""
import argparse
import copy
import json
from pathlib import Path
import xml.etree.ElementTree as ET

import zstandard


def tile(raw):
    if raw in (16,52,88): return {16:'5mr',52:'5pr',88:'5sr'}[raw]
    t=raw//4
    return f'{t%9+1}{"mps"[t//9]}' if t<27 else 'ESWNPFC'[t-27]


def extract(path):
    root=ET.fromstring(zstandard.ZstdDecompressor().decompress(path.read_bytes()))
    events=[]; cases=[]; active=False; last_draw={}; round_number=0
    for node in root:
        tag=node.tag; a=node.attrib
        if tag=='INIT':
            round_number+=1;seed=list(map(int,a['seed'].split(',')));last_draw={}
            hands=[[tile(int(t)) for t in a[f'hai{i}'].split(',')] for i in range(3)]
            events=[dict(type='start_game',names=['A','B','C','']),
                    dict(type='start_kyoku',bakaze='ESWN'[seed[0]//4],kyoku=seed[0]%4+1,
                         honba=seed[1],kyotaku=seed[2],oya=int(a['oya']),
                         dora_marker=tile(seed[5]),scores=[int(t)*100 for t in a['ten'].split(',')],
                         tehais=hands+[[]])]
            events[1]['scores']=(events[1]['scores']+[0]*4)[:4]
            active=events[1]['kyoku']<=3
        elif not active: continue
        elif tag in ('N','AGARI','RYUUKYOKU'):
            active=False
        elif tag=='REACH':
            events.append(dict(type='reach' if a['step']=='1' else 'reach_accepted',actor=int(a['who'])))
        elif tag=='DORA': events.append(dict(type='dora',dora_marker=tile(int(a['hai']))))
        elif len(tag)>1 and tag[0] in 'TUVtuvDEFdef' and tag[1:].isdigit():
            who='TUV'.find(tag[0].upper())
            if who>=0:
                raw=int(tag[1:]);last_draw[who]=raw
                events.append(dict(type='tsumo',actor=who,pai=tile(raw)))
                perspective=copy.deepcopy(events)
                for event in perspective:
                    if event['type']=='start_kyoku':
                        event['tehais']=[hand if i==who else ['?']*13 for i,hand in enumerate(event['tehais'])]
                    elif event['type']=='tsumo' and event['actor']!=who: event['pai']='?'
                cases.append(dict(id=f'{path.stem}-r{round_number}-d{len(cases)}',group='public-prefix',seat=who,events=perspective))
            else:
                who='DEF'.index(tag[0].upper());raw=int(tag[1:])
                events.append(dict(type='dahai',actor=who,pai=tile(raw),tsumogiri=last_draw.get(who)==raw))
    return cases


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('folder',type=Path);p.add_argument('output',type=Path)
    args=p.parse_args()
    cases=[]
    for path in sorted(args.folder.glob('*.xml.zst')):
        items=extract(path);cases.extend(items);print(path.name,len(items))
    args.output.write_text(json.dumps(cases),encoding='utf-8')
    print('Exported',len(cases),'call-free decision prefixes')


if __name__=='__main__': main()
