

%.embed.svg: %.svg
	svg-embed-font $<

all:
	find . | grep '[.]excalidraw$$' | sed 's/[.]excalidraw$$/.embed.svg/' | xargs make
