"""Logo de Tâches : carré bleu nuit arrondi, cercle coché crème.
Usage : py -3 assets/make_icon.py"""
from pathlib import Path
from PIL import Image, ImageDraw
CREAM=(242,230,204,255); NAVY=(19,34,58,255); BLUE=(111,168,255,255)
def draw(px):
    S=4*px; img=Image.new("RGBA",(S,S),(0,0,0,0)); d=ImageDraw.Draw(img); k=S/1024
    s=lambda v:int(v*k)
    d.rounded_rectangle([s(24),s(24),s(1000),s(1000)],radius=s(200),fill=NAVY)
    d.ellipse([s(180),s(180),s(844),s(844)],outline=BLUE,width=max(1,s(70)))
    d.line([(s(340),s(520)),(s(470),s(650)),(s(700),s(390))],fill=CREAM,width=max(1,s(90)),joint="curve")
    return img.resize((px,px),Image.LANCZOS)
here=Path(__file__).parent
draw(256).save(here/"icon.png"); draw(64).save(here/"icon64.png")
draw(256).save(here/"icon.ico",sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
