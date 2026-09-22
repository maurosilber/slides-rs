const slides = document.getElementsByTagName("section");

let currentSlide = 0;
slides[currentSlide].style.display = "block";

function updateSlide(i) {
    if (i < 0 || i >= slides.length) return;
    slides[i].style.display = "block"
    slides[currentSlide].style.display = "none";
    currentSlide = i;
}

addEventListener("keydown", (event) => {
    if (event.code == "ArrowRight") {
        updateSlide(currentSlide + 1)
    } else if (event.code == "ArrowLeft") {
        updateSlide(currentSlide - 1)
    }
})