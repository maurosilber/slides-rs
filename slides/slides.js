const slides = document.getElementsByTagName("section");

// The current slide is kept in the URL, so that the reload after a
// rebuild comes back to it instead of to the first slide.
function slideFromHash() {
    const slide = Number(location.hash.slice(1)) - 1;
    if (!Number.isInteger(slide) || slide < 0) return 0;
    // The deck may have lost the slide we were on.
    return Math.min(slide, slides.length - 1);
}

let currentSlide = slideFromHash();
slides[currentSlide].style.display = "block";
history.replaceState(null, "", "#" + (currentSlide + 1));

function updateSlide(i) {
    if (i < 0 || i >= slides.length) return;
    slides[i].style.display = "block";
    slides[currentSlide].style.display = "none";
    currentSlide = i;
    history.replaceState(null, "", "#" + (i + 1));
}

addEventListener("keydown", (event) => {
    if (event.code == "ArrowRight") {
        updateSlide(currentSlide + 1);
    } else if (event.code == "ArrowLeft") {
        updateSlide(currentSlide - 1);
    }
});

// The URL can also be edited, or gone back through.
addEventListener("hashchange", () => updateSlide(slideFromHash()));
